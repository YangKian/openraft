//! Raft runtime configuration.

use rand::thread_rng;
use rand::Rng;
use serde::Deserialize;
use serde::Serialize;
use structopt::StructOpt;

use crate::error::ConfigError;

/// Log compaction and snapshot policy.
///
/// This governs when periodic snapshots will be taken, and also governs the conditions which
/// would cause a leader to send an `InstallSnapshot` RPC to a follower based on replication lag.
///
/// Additional policies may become available in the future.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SnapshotPolicy {
    /// A snapshot will be generated once the log has grown the specified number of logs since
    /// the last snapshot.
    LogsSinceLast(u64),
}

/// Parse number with unit such as 5.3 KB
fn parse_bytes_with_unit(src: &str) -> anyhow::Result<u64> {
    let res = byte_unit::Byte::from_str(src)?;

    Ok(res.get_bytes() as u64)
}

fn parse_snapshot_policy(src: &str) -> anyhow::Result<SnapshotPolicy> {
    let elts = src.split(':').collect::<Vec<_>>();
    if elts.len() != 2 {
        return Err(anyhow::anyhow!(
            "snapshot policy should be in form of 'since_last:<num>'"
        ));
    }

    if elts[0] != "since_last" {
        return Err(anyhow::anyhow!(
            "snapshot policy should be in form of 'since_last:<num>'"
        ));
    }

    let n_logs = elts[1].parse::<u64>()?;
    Ok(SnapshotPolicy::LogsSinceLast(n_logs))
}

/// The runtime configuration for a Raft node.
///
/// The default values used by this type should generally work well for Raft clusters which will
/// be running with nodes in multiple datacenter availability zones with low latency between
/// zones. These values should typically be made configurable from the perspective of the
/// application which is being built on top of Raft.
///
/// When building the Raft configuration for your application, remember this inequality from the
/// Raft spec: `broadcastTime ≪ electionTimeout ≪ MTBF`.
///
/// > In this inequality `broadcastTime` is the average time it takes a server to send RPCs in
/// > parallel to every server in the cluster and receive their responses; `electionTimeout` is the
/// > election timeout described in Section 5.2; and `MTBF` is the average time between failures for
/// > a single server. The broadcast time should be an order of magnitude less than the election
/// > timeout so that leaders can reliably send the heartbeat messages required to keep followers
/// > from starting elections; given the randomized approach used for election timeouts, this
/// > inequality also makes split votes unlikely. The election timeout should be a few orders of
/// > magnitude less than `MTBF` so that the system makes steady progress. When the leader crashes,
/// > the system will be unavailable for roughly the election timeout; we would like this to
/// > represent only a small fraction of overall time.
///
/// What does all of this mean? Simply keep your election timeout settings high enough that the
/// performance of your network will not cause election timeouts, but don't keep it so high that
/// a real leader crash would cause prolonged downtime. See the Raft spec §5.6 for more details.
#[derive(Clone, Debug, Serialize, Deserialize, StructOpt)]
pub struct Config {
    /// The application specific name of this Raft cluster
    #[structopt(long, env = "RAFT_CLUSTER_NAME", default_value = "foo")]
    pub cluster_name: String,

    /// The minimum election timeout in milliseconds
    #[structopt(long, env = "RAFT_ELECTION_TIMEOUT_MIN", default_value = "150")]
    pub election_timeout_min: u64,

    /// The maximum election timeout in milliseconds
    #[structopt(long, env = "RAFT_ELECTION_TIMEOUT_MAX", default_value = "300")]
    pub election_timeout_max: u64,

    /// The heartbeat interval in milliseconds at which leaders will send heartbeats to followers
    #[structopt(long, env = "RAFT_HEARTBEAT_INTERVAL", default_value = "50")]
    pub heartbeat_interval: u64,

    /// The timeout for sending a snapshot segment, in millisecond
    #[structopt(long, env = "RAFT_INSTALL_SNAPSHOT_TIMEOUT", default_value = "200")]
    pub install_snapshot_timeout: u64,

    /// The maximum number of entries per payload allowed to be transmitted during replication
    ///
    /// If this is too low, it will take longer for the nodes to be brought up to
    /// consistency with the rest of the cluster.
    #[structopt(long, env = "RAFT_MAX_PAYLOAD_ENTRIES", default_value = "300")]
    pub max_payload_entries: u64,

    /// The distance behind in log replication a follower must fall before it is considered lagging
    ///
    /// Once a replication stream transition into line-rate state, the target node will be considered safe to join a
    /// cluster.
    #[structopt(long, env = "RAFT_REPLICATION_LAG_THRESHOLD", default_value = "1000")]
    pub replication_lag_threshold: u64,

    /// The snapshot policy to use for a Raft node.
    #[structopt(
        long,
        env = "RAFT_SNAPSHOT_POLICY",
        default_value = "since_last:5000",
        parse(try_from_str=parse_snapshot_policy)
    )]
    pub snapshot_policy: SnapshotPolicy,

    /// The maximum snapshot chunk size allowed when transmitting snapshots (in bytes)
    #[structopt(long, env = "RAFT_SNAPSHOT_MAX_CHUNK_SIZE", default_value = "3MiB", parse(try_from_str=parse_bytes_with_unit))]
    pub snapshot_max_chunk_size: u64,

    /// The maximum number of applied logs to keep before purging
    #[structopt(long, env = "RAFT_MAX_APPLIED_LOG_TO_KEEP", default_value = "1000")]
    pub max_applied_log_to_keep: u64,
}

impl Default for Config {
    fn default() -> Self {
        <Self as StructOpt>::from_iter(&Vec::<&'static str>::new())
    }
}

impl Config {
    /// Generate a new random election timeout within the configured min & max.
    pub fn new_rand_election_timeout(&self) -> u64 {
        thread_rng().gen_range(self.election_timeout_min..self.election_timeout_max)
    }

    pub fn build(args: &[&str]) -> Result<Config, ConfigError> {
        let config = <Self as StructOpt>::from_iter(args);
        config.validate()
    }

    /// Validate the state of this config.
    pub fn validate(self) -> Result<Config, ConfigError> {
        if self.election_timeout_min >= self.election_timeout_max {
            return Err(ConfigError::InvalidElectionTimeoutMinMax);
        }

        if self.election_timeout_min <= self.heartbeat_interval {
            return Err(ConfigError::ElectionTimeoutLessThanHeartBeatInterval);
        }

        if self.max_payload_entries == 0 {
            return Err(ConfigError::MaxPayloadEntriesTooSmall);
        }

        Ok(self)
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// Unit Tests ////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let cfg = Config::default();

        assert!(cfg.election_timeout_min >= 150);
        assert!(cfg.election_timeout_max <= 300);

        assert_eq!(50, cfg.heartbeat_interval);
        assert_eq!(300, cfg.max_payload_entries);
        assert_eq!(1000, cfg.replication_lag_threshold);

        assert_eq!(3 * 1024 * 1024, cfg.snapshot_max_chunk_size);
        assert_eq!(SnapshotPolicy::LogsSinceLast(5000), cfg.snapshot_policy);
    }

    #[test]
    fn test_invalid_election_timeout_config_produces_expected_error() {
        let config = Config {
            election_timeout_min: 1000,
            election_timeout_max: 700,
            ..Default::default()
        };

        let res = config.validate();
        let err = res.unwrap_err();
        assert_eq!(err, ConfigError::InvalidElectionTimeoutMinMax);
    }

    #[test]
    fn test_build() -> anyhow::Result<()> {
        let config = Config::build(&[
            "foo",
            "--cluster-name=bar",
            "--election-timeout-min=10",
            "--election-timeout-max=20",
            "--heartbeat-interval=5",
            "--install-snapshot-timeout=200",
            "--max-payload-entries=201",
            "--replication-lag-threshold=202",
            "--snapshot-policy=since_last:203",
            "--snapshot-max-chunk-size=204",
            "--max-applied-log-to-keep=205",
        ])?;

        assert_eq!("bar", config.cluster_name);
        assert_eq!(10, config.election_timeout_min);
        assert_eq!(20, config.election_timeout_max);
        assert_eq!(5, config.heartbeat_interval);
        assert_eq!(200, config.install_snapshot_timeout);
        assert_eq!(201, config.max_payload_entries);
        assert_eq!(202, config.replication_lag_threshold);
        assert_eq!(SnapshotPolicy::LogsSinceLast(203), config.snapshot_policy);
        assert_eq!(204, config.snapshot_max_chunk_size);
        assert_eq!(205, config.max_applied_log_to_keep);

        Ok(())
    }
}
