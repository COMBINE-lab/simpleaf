//! Validation and atomic installation of the probe indices that
//! `multiplex-quant` builds automatically and caches under
//! `$ALEVIN_FRY_HOME/probe_indices`.
//!
//! piscem's on-disk index format changes between releases (piscem-rs 0.11, for
//! example, moved to PCTAB04/PECTB04 tables), and piscem refuses an index
//! written by a different release with "... must be rebuilt". A cache slot is
//! therefore reused only when its build record ([`BUILD_RECORD_FILE`]) says it
//! was built by the piscem that is registered now, with the same settings.
//! "The same piscem" means the same version string *and* the same executable
//! file (canonical path, size and modification time): development builds of
//! piscem often still report the last release's version, and an in-place
//! upgrade does not re-run `simpleaf set-paths`. A slot without a record —
//! written by an earlier simpleaf, or left behind by an interrupted build — is
//! rebuilt as well; probe indices build in seconds.
//!
//! An index is built in a private staging directory next to its slot and
//! renamed into place once complete, so a concurrent run never sees a
//! half-built index in the shared cache.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tracing::warn;

use crate::utils::probe_utils::ExcludedProbeMode;
use crate::utils::prog_utils::{self, ProgInfo};

/// File in each cache slot recording what built the index in that slot.
pub const BUILD_RECORD_FILE: &str = "simpleaf_probe_index.json";

/// The piscem executable that built (or would build) an index.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PiscemIdentity {
    /// The version string recorded for piscem by `simpleaf set-paths`.
    pub version: String,
    /// The canonical (symlink-resolved) path of the executable.
    pub exe_path: PathBuf,
    /// The size of the executable, in bytes.
    pub exe_size: u64,
    /// The modification time of the executable.
    pub exe_modified: SystemTime,
}

impl PiscemIdentity {
    /// Identify the registered piscem by inspecting its executable on disk.
    pub fn of(piscem: &ProgInfo) -> anyhow::Result<Self> {
        let exe_path = std::fs::canonicalize(&piscem.exe_path).with_context(|| {
            format!(
                "could not resolve the registered piscem executable {}; please run `simpleaf set-paths`",
                piscem.exe_path.display()
            )
        })?;
        let meta = std::fs::metadata(&exe_path)
            .with_context(|| format!("could not inspect {}", exe_path.display()))?;
        let exe_modified = meta.modified().with_context(|| {
            format!(
                "could not read the modification time of {}",
                exe_path.display()
            )
        })?;
        // The record can only store times from the epoch on.
        let exe_modified = exe_modified.max(SystemTime::UNIX_EPOCH);
        Ok(Self {
            version: piscem.version.clone(),
            exe_path,
            exe_size: meta.len(),
            exe_modified,
        })
    }
}

/// What a cached probe index was built with; stored as [`BUILD_RECORD_FILE`].
/// A cached index is reused only when its record equals the one for the
/// current run. `--dict` is deliberately not part of it: every build writes
/// the sshash dictionary, and `piscem map-sc --dict tiny` derives the tiny one
/// from it in memory.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProbeIndexBuildRecord {
    pub piscem: PiscemIdentity,
    pub kmer_length: usize,
    pub excluded_probes: String,
}

impl ProbeIndexBuildRecord {
    /// The record an index built by this run would carry.
    pub fn for_run(
        piscem: &ProgInfo,
        kmer_length: usize,
        excluded_probes: ExcludedProbeMode,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            piscem: PiscemIdentity::of(piscem)?,
            kmer_length,
            excluded_probes: excluded_probes.as_str().to_string(),
        })
    }

    /// Why an index carrying `self` can not serve a run expecting `expected`,
    /// or `None` if it can.
    fn mismatch(&self, expected: &Self) -> Option<String> {
        if self.piscem != expected.piscem {
            let (built, now) = (&self.piscem, &expected.piscem);
            return Some(
                if built.version == now.version && built.exe_path == now.exe_path {
                    format!(
                        "the piscem executable {} has changed (size or modification time) since it was built",
                        now.exe_path.display()
                    )
                } else {
                    format!(
                        "it was built by piscem {} ({}), but the registered piscem is {} ({})",
                        built.version,
                        built.exe_path.display(),
                        now.version,
                        now.exe_path.display()
                    )
                },
            );
        }
        if self.kmer_length != expected.kmer_length
            || self.excluded_probes != expected.excluded_probes
        {
            return Some(format!(
                "it was built with k={} and --excluded-probes {}, but this run uses k={} and --excluded-probes {}",
                self.kmer_length,
                self.excluded_probes,
                expected.kmer_length,
                expected.excluded_probes
            ));
        }
        None
    }
}

/// The state of a cache slot with respect to the current run.
#[derive(Debug, Eq, PartialEq)]
pub enum SlotStatus {
    /// There is no cached index yet.
    Absent,
    /// The slot holds a complete index built with the expected record.
    Fresh,
    /// The slot holds something that must be rebuilt; the message says why.
    Stale(String),
}

/// Check whether `slot`, whose piscem index is `index_prefix`, can serve a run
/// whose index would carry `expected`.
pub fn slot_status(
    slot: &Path,
    index_prefix: &Path,
    expected: &ProbeIndexBuildRecord,
) -> SlotStatus {
    if !slot.exists() {
        return SlotStatus::Absent;
    }
    let record_path = slot.join(BUILD_RECORD_FILE);
    if !record_path.exists() {
        return SlotStatus::Stale(format!(
            "it has no {BUILD_RECORD_FILE} (it was cached by an earlier simpleaf, or its build did not finish)"
        ));
    }
    let record = std::fs::read_to_string(&record_path)
        .map_err(anyhow::Error::from)
        .and_then(|s| serde_json::from_str::<ProbeIndexBuildRecord>(&s).map_err(Into::into));
    let record = match record {
        Ok(record) => record,
        Err(e) => return SlotStatus::Stale(format!("its {BUILD_RECORD_FILE} is unreadable ({e})")),
    };
    if let Some(reason) = record.mismatch(expected) {
        return SlotStatus::Stale(reason);
    }
    if prog_utils::check_piscem_index_files(index_prefix).is_err() {
        return SlotStatus::Stale("its index files are incomplete".to_string());
    }
    SlotStatus::Fresh
}

/// Create a private directory, next to `slot`, to build its index in. It is
/// removed when dropped unless [`install`] has moved it into place.
pub fn staging_dir(slot: &Path) -> anyhow::Result<tempfile::TempDir> {
    let (cache_dir, slot_name) = split_slot(slot)?;
    tempfile::Builder::new()
        .prefix(&format!(".{slot_name}.tmp-"))
        .tempdir_in(cache_dir)
        .with_context(|| {
            format!(
                "could not create a staging directory in {}",
                cache_dir.display()
            )
        })
}

/// Write `record` as the build record of the index built in `dir`.
pub fn write_build_record(dir: &Path, record: &ProbeIndexBuildRecord) -> anyhow::Result<()> {
    let path = dir.join(BUILD_RECORD_FILE);
    let file = std::fs::File::create(&path)
        .with_context(|| format!("could not create {}", path.display()))?;
    serde_json::to_writer_pretty(file, record)
        .with_context(|| format!("could not write {}", path.display()))?;
    Ok(())
}

/// Move the complete, recorded index built in `staging` into `slot`, first
/// retiring whatever stale index the slot holds. Returns `false`, leaving
/// `staging` in place, if another run has meanwhile installed an index that
/// serves `record` just as well; the caller then uses that one.
pub fn install(
    staging: &Path,
    slot: &Path,
    index_prefix: &Path,
    record: &ProbeIndexBuildRecord,
) -> anyhow::Result<bool> {
    match slot_status(slot, index_prefix, record) {
        SlotStatus::Fresh => return Ok(false),
        SlotStatus::Absent => {}
        SlotStatus::Stale(_) => retire(slot)?,
    }
    match std::fs::rename(staging, slot) {
        Ok(()) => Ok(true),
        // Another run installed its index between our check and the rename.
        Err(_) if slot_status(slot, index_prefix, record) == SlotStatus::Fresh => Ok(false),
        Err(e) => Err(e).with_context(|| {
            format!(
                "could not move the probe index built in {} to {}",
                staging.display(),
                slot.display()
            )
        }),
    }
}

/// Re-express `path`, which lies inside the staging directory `staging`, as
/// the corresponding path inside `slot`.
pub fn rebase(path: PathBuf, staging: &Path, slot: &Path) -> PathBuf {
    match path.strip_prefix(staging) {
        Ok(rel) => slot.join(rel),
        Err(_) => path,
    }
}

/// Move a stale slot out of the way (a single rename, so no run ever sees it
/// half-deleted), then delete it.
fn retire(slot: &Path) -> anyhow::Result<()> {
    let (cache_dir, slot_name) = split_slot(slot)?;
    let graveyard = tempfile::Builder::new()
        .prefix(&format!(".{slot_name}.stale-"))
        .tempdir_in(cache_dir)
        .with_context(|| format!("could not create a directory in {}", cache_dir.display()))?;
    match std::fs::rename(slot, graveyard.path().join(slot_name)) {
        Ok(()) => {}
        // A concurrent run retired it first.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            return Err(e).with_context(|| {
                format!(
                    "could not move the stale probe index {} aside",
                    slot.display()
                )
            });
        }
    }
    let graveyard_path = graveyard.path().to_path_buf();
    if let Err(e) = graveyard.close() {
        warn!(
            "could not remove the stale probe index moved to {}: {e}",
            graveyard_path.display()
        );
    }
    Ok(())
}

fn split_slot(slot: &Path) -> anyhow::Result<(&Path, &str)> {
    let cache_dir = slot
        .parent()
        .with_context(|| format!("{} has no parent directory", slot.display()))?;
    let slot_name = slot
        .file_name()
        .and_then(|n| n.to_str())
        .with_context(|| format!("{} has no usable file name", slot.display()))?;
    Ok((cache_dir, slot_name))
}

#[cfg(test)]
mod tests {
    use super::{
        BUILD_RECORD_FILE, PiscemIdentity, ProbeIndexBuildRecord, SlotStatus, install, rebase,
        slot_status, staging_dir, write_build_record,
    };
    use crate::utils::probe_utils::ExcludedProbeMode;
    use crate::utils::prog_utils::ProgInfo;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime};
    use tempfile::tempdir;

    fn fake_piscem(dir: &Path, contents: &str) -> ProgInfo {
        let exe_path = dir.join("piscem");
        fs::write(&exe_path, contents).expect("failed to write fake piscem");
        ProgInfo {
            exe_path,
            version: "0.24.0".to_string(),
        }
    }

    /// Lay out a complete cache slot the way `multiplex-quant` builds one, and
    /// return its index prefix.
    fn write_slot(slot: &Path, record: Option<&ProbeIndexBuildRecord>) -> PathBuf {
        let index_dir = slot.join("probe_index");
        fs::create_dir_all(&index_dir).expect("failed to create slot");
        let prefix = index_dir.join("index");
        for ext in ["ctab", "refinfo", "ssi", "ssi.mphf"] {
            fs::write(prefix.with_extension(ext), "").expect("failed to write index file");
        }
        fs::write(index_dir.join("probe_t2g.tsv"), "P1\tG1\n").expect("failed to write t2g");
        if let Some(record) = record {
            write_build_record(slot, record).expect("failed to write record");
        }
        prefix
    }

    fn assert_stale(status: SlotStatus, needle: &str) {
        match status {
            SlotStatus::Stale(reason) => {
                assert!(reason.contains(needle), "unexpected reason: {reason}")
            }
            other => panic!("expected a stale slot ({needle}), got {other:?}"),
        }
    }

    #[test]
    fn identity_tracks_version_path_and_file() {
        let td = tempdir().expect("failed to create tempdir");
        let piscem = fake_piscem(td.path(), "piscem build 1");
        let id = PiscemIdentity::of(&piscem).expect("failed to identify piscem");
        assert_eq!(id.version, "0.24.0");
        assert_eq!(id.exe_size, 14);
        assert_eq!(id.exe_path, fs::canonicalize(&piscem.exe_path).unwrap());

        // A symlink to the same executable is the same piscem.
        #[cfg(unix)]
        {
            let link = td.path().join("piscem-link");
            std::os::unix::fs::symlink(&piscem.exe_path, &link).expect("failed to symlink");
            let via_link = ProgInfo {
                exe_path: link,
                version: piscem.version.clone(),
            };
            assert_eq!(PiscemIdentity::of(&via_link).unwrap(), id);
        }

        assert!(
            PiscemIdentity::of(&ProgInfo {
                exe_path: td.path().join("missing"),
                version: "0.24.0".to_string(),
            })
            .is_err()
        );
    }

    #[test]
    fn matching_record_is_fresh_and_absent_slot_is_absent() {
        let td = tempdir().expect("failed to create tempdir");
        let piscem = fake_piscem(td.path(), "piscem");
        let record = ProbeIndexBuildRecord::for_run(&piscem, 23, ExcludedProbeMode::Decoy)
            .expect("failed to build record");
        let slot = td.path().join("probe_indices").join("plist_23_decoy");
        let prefix = slot.join("probe_index").join("index");
        assert_eq!(slot_status(&slot, &prefix, &record), SlotStatus::Absent);

        write_slot(&slot, Some(&record));
        assert_eq!(slot_status(&slot, &prefix, &record), SlotStatus::Fresh);

        // The record round-trips exactly through its JSON file.
        let on_disk: ProbeIndexBuildRecord =
            serde_json::from_str(&fs::read_to_string(slot.join(BUILD_RECORD_FILE)).unwrap())
                .unwrap();
        assert_eq!(on_disk, record);
    }

    #[test]
    fn slot_without_record_is_stale() {
        // What every slot cached by an earlier simpleaf, or left by an
        // interrupted build, looks like.
        let td = tempdir().expect("failed to create tempdir");
        let piscem = fake_piscem(td.path(), "piscem");
        let record = ProbeIndexBuildRecord::for_run(&piscem, 23, ExcludedProbeMode::Ignore)
            .expect("failed to build record");
        let slot = td.path().join("plist_23");
        let prefix = write_slot(&slot, None);
        assert_stale(
            slot_status(&slot, &prefix, &record),
            "no simpleaf_probe_index.json",
        );

        fs::write(slot.join(BUILD_RECORD_FILE), "{not json").unwrap();
        assert_stale(slot_status(&slot, &prefix, &record), "unreadable");
    }

    #[test]
    fn different_piscem_is_stale() {
        let td = tempdir().expect("failed to create tempdir");
        let piscem = fake_piscem(td.path(), "piscem");
        let record = ProbeIndexBuildRecord::for_run(&piscem, 23, ExcludedProbeMode::Decoy)
            .expect("failed to build record");
        let slot = td.path().join("plist_23_decoy");
        let prefix = write_slot(&slot, Some(&record));

        // A newer release.
        let mut newer = record.clone();
        newer.piscem.version = "0.25.0".to_string();
        assert_stale(
            slot_status(&slot, &prefix, &newer),
            "built by piscem 0.24.0",
        );

        // Another piscem install reporting the same version.
        let other_dir = td.path().join("other");
        fs::create_dir_all(&other_dir).unwrap();
        let other = ProbeIndexBuildRecord::for_run(
            &fake_piscem(&other_dir, "piscem"),
            23,
            ExcludedProbeMode::Decoy,
        )
        .unwrap();
        assert_stale(
            slot_status(&slot, &prefix, &other),
            "but the registered piscem is 0.24.0",
        );

        // The same path and version, but the executable was replaced (an
        // unbumped development build, or an in-place upgrade).
        let mut rebuilt = record.clone();
        rebuilt.piscem.exe_size += 1;
        assert_stale(slot_status(&slot, &prefix, &rebuilt), "has changed");
        let mut touched = record.clone();
        touched.piscem.exe_modified += Duration::from_secs(1);
        assert_stale(slot_status(&slot, &prefix, &touched), "has changed");
    }

    #[test]
    fn different_settings_or_missing_files_are_stale() {
        let td = tempdir().expect("failed to create tempdir");
        let piscem = fake_piscem(td.path(), "piscem");
        let record = ProbeIndexBuildRecord::for_run(&piscem, 23, ExcludedProbeMode::Decoy)
            .expect("failed to build record");
        let slot = td.path().join("plist_23_decoy");
        let prefix = write_slot(&slot, Some(&record));

        let mut other_k = record.clone();
        other_k.kmer_length = 25;
        assert_stale(slot_status(&slot, &prefix, &other_k), "k=23");
        let ignore = ProbeIndexBuildRecord::for_run(&piscem, 23, ExcludedProbeMode::Ignore)
            .expect("failed to build record");
        assert_stale(
            slot_status(&slot, &prefix, &ignore),
            "--excluded-probes decoy",
        );

        fs::remove_file(prefix.with_extension("ctab")).unwrap();
        assert_stale(slot_status(&slot, &prefix, &record), "incomplete");
    }

    #[test]
    fn install_moves_staging_into_an_absent_slot() {
        let td = tempdir().expect("failed to create tempdir");
        let piscem = fake_piscem(td.path(), "piscem");
        let record = ProbeIndexBuildRecord::for_run(&piscem, 23, ExcludedProbeMode::Decoy)
            .expect("failed to build record");
        let cache = td.path().join("probe_indices");
        fs::create_dir_all(&cache).unwrap();
        let slot = cache.join("plist_23_decoy");
        let prefix = slot.join("probe_index").join("index");

        let staging = staging_dir(&slot).expect("failed to create staging dir");
        assert_eq!(staging.path().parent(), Some(cache.as_path()));
        let staged_prefix = write_slot(staging.path(), Some(&record));
        assert_eq!(rebase(staged_prefix, staging.path(), &slot), prefix);
        // Paths outside the staging directory are left alone.
        let outside = td.path().join("elsewhere");
        assert_eq!(rebase(outside.clone(), staging.path(), &slot), outside);

        assert!(install(staging.path(), &slot, &prefix, &record).expect("install failed"));
        assert!(!staging.path().exists());
        assert_eq!(slot_status(&slot, &prefix, &record), SlotStatus::Fresh);
        drop(staging);
        // Only the installed slot is left: no staging directory, no leftovers.
        let names: Vec<_> = fs::read_dir(&cache)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("plist_23_decoy")]);
    }

    #[test]
    fn install_replaces_a_stale_slot() {
        let td = tempdir().expect("failed to create tempdir");
        let piscem = fake_piscem(td.path(), "piscem");
        let record = ProbeIndexBuildRecord::for_run(&piscem, 23, ExcludedProbeMode::Ignore)
            .expect("failed to build record");
        let cache = td.path().join("probe_indices");
        let slot = cache.join("plist_23");
        // A slot cached by an earlier simpleaf: no record, and an extra file
        // that must not survive the rebuild.
        let prefix = write_slot(&slot, None);
        fs::write(slot.join("probe_index").join("index_ver.json"), "old").unwrap();

        let staging = staging_dir(&slot).expect("failed to create staging dir");
        write_slot(staging.path(), Some(&record));
        assert!(install(staging.path(), &slot, &prefix, &record).expect("install failed"));
        assert_eq!(slot_status(&slot, &prefix, &record), SlotStatus::Fresh);
        assert!(!slot.join("probe_index").join("index_ver.json").exists());
        drop(staging);
        let names: Vec<_> = fs::read_dir(&cache)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("plist_23")]);
    }

    #[test]
    fn install_defers_to_an_equivalent_index_installed_concurrently() {
        let td = tempdir().expect("failed to create tempdir");
        let piscem = fake_piscem(td.path(), "piscem");
        let record = ProbeIndexBuildRecord::for_run(&piscem, 23, ExcludedProbeMode::Decoy)
            .expect("failed to build record");
        let cache = td.path().join("probe_indices");
        fs::create_dir_all(&cache).unwrap();
        let slot = cache.join("plist_23_decoy");
        let prefix = slot.join("probe_index").join("index");

        // Two runs build at once; the first to finish installs its index.
        let first = staging_dir(&slot).unwrap();
        let second = staging_dir(&slot).unwrap();
        assert_ne!(first.path(), second.path());
        write_slot(first.path(), Some(&record));
        write_slot(second.path(), Some(&record));
        fs::write(first.path().join("winner"), "").unwrap();

        assert!(install(first.path(), &slot, &prefix, &record).unwrap());
        assert!(!install(second.path(), &slot, &prefix, &record).unwrap());
        // The winner's index is kept, and the loser's staging dir goes away
        // with its guard.
        assert!(slot.join("winner").exists());
        assert!(second.path().exists());
        let second_path = second.path().to_path_buf();
        drop(second);
        assert!(!second_path.exists());
        assert_eq!(slot_status(&slot, &prefix, &record), SlotStatus::Fresh);
    }

    #[test]
    fn record_json_is_stable() {
        // The on-disk shape that later simpleaf releases will read back.
        let record = ProbeIndexBuildRecord {
            piscem: PiscemIdentity {
                version: "0.24.0".to_string(),
                exe_path: PathBuf::from("/opt/piscem/bin/piscem"),
                exe_size: 1234,
                exe_modified: SystemTime::UNIX_EPOCH + Duration::new(1_700_000_000, 5),
            },
            kmer_length: 23,
            excluded_probes: "decoy".to_string(),
        };
        let value = serde_json::to_value(&record).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "piscem": {
                    "version": "0.24.0",
                    "exe_path": "/opt/piscem/bin/piscem",
                    "exe_size": 1234,
                    "exe_modified": {"secs_since_epoch": 1_700_000_000u64, "nanos_since_epoch": 5}
                },
                "kmer_length": 23,
                "excluded_probes": "decoy"
            })
        );
    }
}
