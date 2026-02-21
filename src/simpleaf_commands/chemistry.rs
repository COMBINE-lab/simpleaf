use crate::core::io::write_json_pretty_atomic;
use crate::utils::chem_utils::{
    custom_chem_hm_into_json, get_custom_chem_hm, get_single_custom_chem_from_file,
    CustomChemistry, CustomChemistryMap, ExpectedOri, LOCAL_PL_PATH_KEY, REMOTE_PL_URL_KEY,
};
use crate::utils::constants::*;
use crate::utils::prog_utils::{self, download_to_file_compute_hash};
use crate::utils::{self, af_utils::*};
use regex::Regex;

use anyhow::{bail, Context, Result};
use semver::Version;
use serde_json::json;
use serde_json::{Map, Value};
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};
use utils::prog_utils::read_json_from_remote_url;
use utils::remote::is_remote_url;

fn removable_permit_lists(
    used_pls: &HashSet<PathBuf>,
    present_pls: &HashSet<PathBuf>,
) -> HashSet<PathBuf> {
    present_pls - used_pls
}

/// Parse a chemistry version string with additional operation context.
fn parse_chemistry_version(version: &str, context: &str) -> Result<Version> {
    Version::parse(version).with_context(|| {
        format!(
            "Could not parse version {} while {}. Please follow https://semver.org/ (e.g. 0.1.0).",
            version, context
        )
    })
}

/// Return `true` if a new chemistry entry should replace an existing one.
///
/// Replacement happens when `force` is set or when the incoming version is
/// strictly newer than the existing version.
fn should_replace_registry_entry(existing: &Value, incoming: &Value, force: bool) -> Result<bool> {
    if force {
        return Ok(true);
    }
    let current_version = existing
        .get("version")
        .and_then(Value::as_str)
        .with_context(|| "Chemistry should have a string `version` field.")?;
    let new_version = incoming
        .get("version")
        .and_then(Value::as_str)
        .with_context(|| "Chemistry should have a string `version` field.")?;
    Ok(
        parse_chemistry_version(new_version, "comparing incoming chemistry")?
            > parse_chemistry_version(current_version, "comparing existing chemistry")?,
    )
}

/// Merge incoming registry entries into an existing registry object.
///
/// Missing keys are inserted. Existing keys are replaced only when
/// `should_replace_registry_entry` permits it.
fn merge_registry_entries(
    existing: &mut Map<String, Value>,
    incoming: &Map<String, Value>,
    force: bool,
    dry_run_pref: &str,
) -> Result<()> {
    for (k, v) in incoming {
        match existing.get_mut(k) {
            None => {
                existing.insert(k.clone(), v.clone());
            }
            Some(curr) => {
                if should_replace_registry_entry(curr, v, force)? {
                    info!("{}updating {}", dry_run_pref, k);
                    existing.insert(k.clone(), v.clone());
                }
            }
        }
    }
    Ok(())
}

/// Merge entries from deprecated `custom_chemistries.json` into the main registry.
///
/// Only missing entries are inserted, and only when the deprecated value is a
/// valid geometry string.
fn merge_deprecated_registry_entries(
    existing: &mut Map<String, Value>,
    deprecated: &Map<String, Value>,
    dry_run_pref: &str,
) {
    for (k, v) in deprecated {
        if existing.contains_key(k) {
            warn!(
                "{}The main registry already contained the chemistry \"{}\"; Ignored the one from the deprecated registry.",
                dry_run_pref, k
            );
        } else if let Value::String(geom) = v {
            if validate_geometry(geom).is_ok() {
                let new_ent = json!({
                    "geometry": geom,
                    "expected_ori": "both",
                    "version" : CustomChemistry::default_version(),
                });
                existing.insert(k.to_owned(), new_ent);
                info!(
                    "{}Successfully inserted chemistry \"{}\" from the deprecated registry into the main registry.",
                    dry_run_pref, k
                );
            } else {
                warn!(
                    "{}The chemistry \"{}\" in the deprecated registry is not a valid geometry string; Skipped.",
                    dry_run_pref, k
                );
            }
        } else {
            warn!(
                "{}The chemistry \"{}\" in the deprecated registry is not a string; Skipped.",
                dry_run_pref, k
            );
        }
    }
}

/// Persist a JSON value as pretty-printed text to disk.
fn write_json_pretty(path: &Path, value: &Value) -> Result<()> {
    write_json_pretty_atomic(path, value)
        .with_context(|| format!("Could not write {}", path.display()))
}

/// Attempt to get the chemistry definition from the provided JSON file
/// Check if the JSON file is local or remote. If remote, fetch the file first.
/// Parse the JSON file, and look for the specific chemistry with the requested name.
///
/// Returns a tuple providing
///     1. whether or not an attempt should be made to fetch a permit list for this chemistry
///     2. the path to a local permit list file if it already exists
///     3. any optional metadata (i.e. the "meta" field) associated with this chemistry definition.
fn get_chem_def_from_json(
    json_src: &str,
    af_home_path: &Path,
    add_opts: &mut crate::simpleaf_commands::ChemistryAddOpts,
) -> Result<(bool, Option<String>, Option<serde_json::Value>)> {
    let need_fetch_pl;
    let local_plist;

    let source_chem = if is_remote_url(json_src) {
        let chem_hm: CustomChemistryMap =
            serde_json::from_value(read_json_from_remote_url(json_src)?)?;
        if let Some(chem) = chem_hm.get(&add_opts.name) {
            let mut custom_chem = chem.clone();
            custom_chem.name = add_opts.name.to_owned();
            custom_chem
        } else {
            bail!(
                "Could not find chemistry definition for {} from the requested JSON {}",
                &add_opts.name,
                json_src
            );
        }
    } else {
        let json_path = std::path::Path::new(&json_src);
        if let Some(chem) =
            utils::chem_utils::get_single_custom_chem_from_file(json_path, &add_opts.name)?
        {
            chem
        } else {
            bail!(
                "Could not properly parse the chemistry {} from the requested source JSON {}",
                &add_opts.name,
                json_src
            );
        }
    };

    add_opts.geometry = Some(source_chem.geometry().to_owned());
    add_opts.expected_ori = Some(source_chem.expected_ori().as_str().to_owned());
    add_opts.remote_url = source_chem.remote_pl_url().clone();
    add_opts.version = Some(source_chem.version().clone());
    let meta = source_chem.meta().clone();

    if let Some(plist_name) = source_chem.plist_name().clone().map(PathBuf::from) {
        // check if the permit list is already one we have
        let plist_path = af_home_path.join("plist").join(&plist_name);
        local_plist = if plist_path.is_file() {
            debug!(
                "found permit list at {}, will not attempt to copy or download it.",
                plist_path.display()
            );
            need_fetch_pl = false;
            Some(plist_name.display().to_string())
        } else {
            add_opts.local_url = None;
            need_fetch_pl = true;
            None
        };
    } else {
        local_plist = None;
        add_opts.local_url = None;
        need_fetch_pl = true;
    }
    Ok((need_fetch_pl, local_plist, meta))
}

/// Adds a chemistry to the local registry. The user provides a name,
/// a quoted geometry string, and an expected orientation, and optionally a local path and / or a remote-url pointing to the barcode permit list.  
///
/// If a local-url is provided, the Blake3 hash of the corresponding file is
/// computed and that file is copied to `ALEVIN_FRY_HOME/plist` under the name
/// of the content hash.  
///
/// If a remote-url (but not a local one) is provided, the file is downloaded
/// from the remote-url and placed into a file named by the Blake3 hash of
/// the contents, and the remote-url is recorded.
///
/// Finally, if a local and remote-url are both provided, the file is copied
/// from the local-url but the remote-url is recorded.
///
/// The add_chemitry function is also used to update (i.e. overwite) existing
/// chemistry definitions with new ones having the same name. However, an existing
/// chemistry definition will only be overwritten if the newly-provided chemistry
/// is given a strictly greater version number.
///
/// *NOTE*: This function is *eager* --- any file will be copied or downloaded
/// immediately, so it requires a network connection for remote-urls.
pub fn add_chemistry(
    af_home_path: PathBuf,
    mut add_opts: crate::simpleaf_commands::ChemistryAddOpts,
) -> Result<()> {
    let meta: Option<serde_json::Value>;
    let need_fetch_pl;
    let mut local_plist = None;

    if let Some(json_src) = add_opts.from_json.clone() {
        // try to get the chemistry entry from the provided JSON source
        match get_chem_def_from_json(&json_src, &af_home_path, &mut add_opts) {
            // successful
            Ok((fpl, lplist, m)) => {
                // we can't bind declared variables directly in the match
                // so we do this instead.
                (need_fetch_pl, local_plist, meta) = (fpl, lplist, m);
                debug!(
                    "obtained chemistry definition from provided JSON source : {}",
                    json_src
                );
            }
            // failure (explain why)
            anyhow::Result::Err(e) => {
                bail!(
                    "failed to obtain the chemistry definition from the provided JSON source : {}. Error :: {:#}",
                    json_src, e
                );
            }
        }
    } else {
        // if no provided JSON source, then meta is None and we need to
        // try and get the permit list
        meta = None;
        need_fetch_pl = true;
    }

    let geometry = add_opts
        .geometry
        .expect("geometry must be set if not providing a --from-json chemistry");
    // check geometry string, if no good then
    // propagate error.
    validate_geometry(&geometry)?;

    let version = add_opts
        .version
        .unwrap_or(CustomChemistry::default_version());
    let add_ver = parse_chemistry_version(version.as_ref(), "adding chemistry")?;

    let name = add_opts.name;

    // read in the custom chemistry file
    let chem_p = af_home_path.join(CHEMISTRIES_PATH);

    if let Some(existing_entry) = get_single_custom_chem_from_file(&chem_p, &name)? {
        let existing_ver_str = existing_entry.version();
        let existing_ver = parse_chemistry_version(
            existing_ver_str,
            "reading existing chemistry version from chemistries.json",
        )?;
        if add_ver <= existing_ver {
            info!("Attempting to add chemistry with version {:#} which is <= than the existing version ({:#}) for this chemistry; Skipping addition.", add_ver, existing_ver);
            return Ok(());
        } else {
            info!(
                "Updating existing version {:#} of chemistry {} to {:#}",
                existing_ver, name, add_ver
            );
        }
    }

    if need_fetch_pl {
        if let Some(local_url) = add_opts.local_url {
            if local_url.is_file() {
                let metadata = std::fs::metadata(&local_url)?;
                let flen = metadata.size();
                if flen > 4_294_967_296 {
                    warn!("The file provided to local-url ({}) is {:.1} GB. This file will be *copied* into the ALEVIN_FRY_HOME directory", local_url.display(), flen / 1_073_741_824);
                }

                let mut hasher = blake3::Hasher::new();
                hasher.update_mmap(&local_url)?;
                let content_hash = hasher.finalize();
                let hash_str = content_hash.to_string();

                info!(
                    "The provided permit list file {}, had Blake3 hash {}",
                    local_url.display(),
                    hash_str
                );

                let local_plist_name = PathBuf::from(hash_str);
                let pdir = af_home_path.join("plist");
                let local_plist_path = pdir.join(&local_plist_name);

                create_dir_if_absent(&pdir)?;

                // check if the file already exists
                if local_plist_path.is_file() {
                    info!(
                        "Found a content-equivalent permit list file; will use the existing file."
                    );
                } else {
                    info!(
                        "Copying {} to {}",
                        local_url.display(),
                        local_plist_path.display()
                    );
                    std::fs::copy(&local_url, &local_plist_path).with_context(|| {
                        format!(
                            "Failed to copy local permit list url {} to location {}",
                            local_url.display(),
                            local_plist_path.display()
                        )
                    })?;
                }
                local_plist = Some(local_plist_name.display().to_string());
            } else {
                bail!(
                    "The provided local path does not point to a file: {}; cannot proceed.",
                    local_url.display()
                );
            }
        } else if let Some(ref remote_url) = add_opts.remote_url {
            let pdir = af_home_path.join("plist");
            create_dir_if_absent(&pdir)?;

            let tmpfile = {
                let mut h = blake3::Hasher::new();
                h.update(remote_url.as_bytes());
                let hv = h.finalize();
                pdir.join(PathBuf::from(hv.to_string()))
            };

            let hash = download_to_file_compute_hash(remote_url, &tmpfile)?;
            let hash_str = hash.to_string();
            let local_plist_name = PathBuf::from(&hash_str);
            let local_plist_path = pdir.join(&local_plist_name);

            // check if the file already exists
            if local_plist_path.is_file() {
                info!(
                "Found a cached, content-equivalent permit list file; will use the existing file."
            );

                // remove what we just downloaded
                fs::remove_file(tmpfile)?;
            } else {
                info!("Copying {} to {}", remote_url, local_plist_path.display());
                std::fs::rename(tmpfile, local_plist_path)?;
            }
            local_plist = Some(hash_str);
        } else {
            local_plist = None;
        }
    }

    let ori_str = add_opts
        .expected_ori
        .expect("Expected ori must be set if not providing a chemistry using --from-json");
    let ori = ExpectedOri::from_str(&ori_str)?;

    // init the custom chemistry struct
    let custom_chem = CustomChemistry {
        name,
        geometry,
        expected_ori: ori,
        plist_name: local_plist,
        remote_pl_url: add_opts.remote_url,
        version,
        meta,
    };

    let mut chem_hm = get_custom_chem_hm(&chem_p)?;

    // check if the chemistry already exists and log
    if let Some(cc) = chem_hm.get(custom_chem.name()) {
        info!("Chemistry {} is already registered, with geometry {} the one recorded: {}; overwriting geometry specification.", custom_chem.name(), if cc.geometry() == custom_chem.geometry() {"same as"} else {"different with"}, cc.geometry());
        chem_hm
            .entry(custom_chem.name().to_string())
            .and_modify(|e| *e = custom_chem);
    } else {
        info!(
            "Inserting chemistry {} with geometry {}",
            custom_chem.name(),
            custom_chem.geometry()
        );
        chem_hm.insert(custom_chem.name().to_string(), custom_chem);
    }

    // convert the custom chemistry hashmap to json
    let v = custom_chem_hm_into_json(chem_hm)?;
    write_json_pretty(&chem_p, &v)?;

    Ok(())
}

/// Obtains the latest `chemistries.json` from the simpleaf repository.  For each
/// chemistry in that file, it looks the corresponding key up in the user's local
/// `chemistries.json`.  If a corresponding entry is found, it replaces the entry
/// if the remote entry's version is stricly greater than the local version.
/// Otherwise, it retains the local version.  Any chemistries that are not present
/// in the remote file remain unmodified.
pub fn refresh_chemistries(
    af_home: PathBuf,
    refresh_opts: crate::simpleaf_commands::ChemistryRefreshOpts,
) -> Result<()> {
    let dry_run = refresh_opts.dry_run;
    let dry_run_pref = if dry_run { "[dry_run] : " } else { "" };
    let dry_run_dir = af_home.join("plist_dryrun");

    // if the old custom chem file exists, then warn the user about it
    // but read it in and attempt to populate.
    let custom_chem_file = af_home.join(CUSTOM_CHEMISTRIES_PATH);
    let merge_custom_chem = if custom_chem_file.exists() {
        warn!("{}Found deprecated chemistry registry file \"{}\"; Attempting to merge the chemistries defined in this file into the main registry.", dry_run_pref, CUSTOM_CHEMISTRIES_PATH);
        true
    } else {
        false
    };

    let chem_path = af_home.join(CHEMISTRIES_PATH);
    let fresh_download = if !chem_path.is_file() {
        prog_utils::download_to_file(CHEMISTRIES_URL, &chem_path)?;
        true
    } else {
        false
    };

    // check if the chemistry file is absent altogether
    // if so, then download it
    let chem_path = if dry_run {
        std::fs::create_dir_all(&dry_run_dir).with_context(|| {
            format!(
                "Could not create dry run directory {}",
                dry_run_dir.display()
            )
        })?;
        let dry_run_chem_path = dry_run_dir.join(CHEMISTRIES_PATH);
        std::fs::copy(chem_path, &dry_run_chem_path)?;
        dry_run_chem_path
    } else {
        af_home.join(CHEMISTRIES_PATH)
    };

    // if it's a dry-run, copy over the custom chems if we have one
    let custom_chem_file = if merge_custom_chem && dry_run {
        let p = dry_run_dir.join(CUSTOM_CHEMISTRIES_PATH);
        std::fs::copy(custom_chem_file, &p)?;
        p
    } else {
        custom_chem_file
    };

    if !fresh_download {
        let tmp_chem_path = chem_path.with_extension("tmp.json");
        prog_utils::download_to_file(CHEMISTRIES_URL, &tmp_chem_path)?;
        if let Some(existing_chem) = parse_resource_json_file(&chem_path, None)?.as_object_mut() {
            if let Some(new_chem) = parse_resource_json_file(&tmp_chem_path, None)?.as_object() {
                merge_registry_entries(existing_chem, new_chem, refresh_opts.force, dry_run_pref)?;
                write_json_pretty(&chem_path, &Value::Object(existing_chem.clone()))?;

                // remove the temp file
                std::fs::remove_file(tmp_chem_path)?;
            } else {
                bail!("Could not parse the main registry from \"{}\" file. Please report this on GitHub.", chem_path.display());
            }
        } else {
            bail!(
                "Could not parse the main registry from \"{}\" file. Please report this on GitHub.",
                chem_path.display()
            );
        }
    }

    if merge_custom_chem {
        if let Some(new_chem) = parse_resource_json_file(&chem_path, None)?.as_object_mut() {
            if let Some(old_custom_chem) =
                parse_resource_json_file(&custom_chem_file, None)?.as_object()
            {
                merge_deprecated_registry_entries(new_chem, old_custom_chem, dry_run_pref);
                write_json_pretty(&chem_path, &Value::Object(new_chem.clone()))?;

                let backup = custom_chem_file.with_extension("json.bak");
                std::fs::rename(custom_chem_file, backup)?;
            } else {
                bail!("Could not parse the deprecated registry file as a JSON object; it may be corrupted. Consider deleting this file from {}.", custom_chem_file.display());
            }
        } else {
            bail!("Could not parse the main chemistry registry file, \"{}\", as a JSON object. Please report this on GitHub.", chem_path.display());
        }
    }

    // if it's a dry run, remove the whole directory we created
    if dry_run {
        std::fs::remove_dir_all(&dry_run_dir).with_context(|| {
            format!(
                "couldn't remove the dry run directory {}",
                dry_run_dir.display()
            )
        })?;
    } else {
        info!("Successfully refreshed the chemistry registry.");
    }

    Ok(())
}

/// Finds the set of files (A) listed in `ALEVIN_FRY_HOME/plist` (where permit list files live)
/// and the set of files (B) listed in `chemistries.json` (all entries corresponding to a
/// `plist_name` entry).  It then computes C = A - B, the set of currently unused permit list
/// files, and removes them (or lists them if remove_opts has dry_run set).
pub fn clean_chemistries(
    af_home_path: PathBuf,
    clean_opts: crate::simpleaf_commands::ChemistryCleanOpts,
) -> Result<()> {
    let dry_run = clean_opts.dry_run;

    // read in the custom chemistry file
    let chem_p = af_home_path.join(CHEMISTRIES_PATH);
    let plist_path = af_home_path.join("plist");
    if !plist_path.is_dir() {
        info!(
            "The permit list cache directory {} does not exist; Nothing to clean.",
            plist_path.display()
        );
        return Ok(());
    }

    let chem_hm = get_custom_chem_hm(&chem_p)?;

    let used_pls = chem_hm
        .values()
        .filter_map(|v| v.plist_name().as_ref().map(|s| plist_path.join(s)))
        .collect::<HashSet<PathBuf>>();

    let present_pls = std::fs::read_dir(&plist_path)?
        .filter_map(|de| {
            if let Ok(entry) = de {
                let path = entry.path();
                if path.is_file() {
                    Some(path)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect::<HashSet<PathBuf>>();

    let rem_pls = removable_permit_lists(&used_pls, &present_pls);

    // check if the chemistry already exists and log
    if dry_run {
        if rem_pls.is_empty() {
            info!("[dry_run] : No permit list files in the cache directory are currently unused; Nothing to clean.");
        } else {
            info!("[dry_run] : The following files in the permit list directory do not match any registered chemistries and would be removed: {:#?}", rem_pls);
        }
    } else {
        for pl in &rem_pls {
            info!("removing file from {}", pl.display());
            std::fs::remove_file(pl)?;
        }
    }

    Ok(())
}

/// Remove the entry (or entries matching the provided regex) for the provided chemistry in `chemistries.json` if it is present.
pub fn remove_chemistry(
    af_home_path: PathBuf,
    remove_opts: crate::simpleaf_commands::ChemistryRemoveOpts,
) -> Result<()> {
    let name = remove_opts.name;
    // read in the custom chemistry file
    let chem_p = af_home_path.join(CHEMISTRIES_PATH);

    let mut chem_hm = get_custom_chem_hm(&chem_p)?;
    let mut num_matched = 0;
    let keys = chem_hm.keys().cloned().collect::<Vec<String>>();

    if let Ok(name_re) = regex::Regex::new(&name) {
        for k in keys {
            if name_re.is_match(&k) {
                num_matched += 1;
                if remove_opts.dry_run {
                    info!(
                        "[dry_run] : Would remove chemistry \"{}\" from the registry.",
                        k
                    );
                } else {
                    info!("Chemistry \"{}\" found in the registry; Removing it!", k);
                    chem_hm.remove(&k);
                }
            }
        }
    } else {
        bail!(
            "The provided chemistry name {} was neither a valid chemistry name nor a valid regex.",
            name
        );
    }

    if num_matched == 0 {
        info!(
            "No chemistry with name \"{}\" (or matching this as a regex) was found in the registry; nothing to remove.",
            name
        );
    } else if !remove_opts.dry_run {
        // convert the custom chemistry hashmap to json
        let v = custom_chem_hm_into_json(chem_hm)?;
        write_json_pretty(&chem_p, &v)?;
    }

    Ok(())
}

/// Lookup the chemistry, or the chemistries matching the provided regex in the
/// chemistry registry.
pub fn lookup_chemistry(
    af_home_path: PathBuf,
    lookup_opts: crate::simpleaf_commands::ChemistryLookupOpts,
) -> Result<()> {
    let name = lookup_opts.name;
    // read in the custom chemistry file
    let chem_p = af_home_path.join(CHEMISTRIES_PATH);

    // check if the chemistry already exists and log
    if let Some(cc) = get_single_custom_chem_from_file(&chem_p, &name)? {
        println!("=================");
        print!("{}", cc);
        println!("=================");
    } else {
        info!("No chemistry with name {} was found in the registry!", name);
        info!(
            "Treating {} as a regex and searching for matching chemistries",
            name
        );
        let chem_hm = get_custom_chem_hm(&chem_p)?;

        if let Ok(re) = Regex::new(&name) {
            println!("=================");
            for (cname, cval) in chem_hm.iter() {
                if re.is_match(cname) {
                    print!("{}", cval);
                    println!("=================");
                }
            }
        } else {
            info!(
                "No chemistry matching regex pattern {} was found in the registry!",
                name
            );
        }
    }

    Ok(())
}

struct FetchSet<'a> {
    pub m: HashSet<&'a String>,
    pub re: Option<Regex>,
}

impl<'a> FetchSet<'a> {
    pub fn from_re(s: &str) -> Result<Self> {
        if let Ok(re) = regex::Regex::new(s) {
            Ok(Self {
                m: HashSet::new(),
                re: Some(re),
            })
        } else {
            bail!("Could not compile regex : [{}]", s)
        }
    }

    pub fn from_hash_set(m: HashSet<&'a String>) -> Self {
        Self { m, re: None }
    }

    pub fn contains(&self, k: &String) -> bool {
        if let Some(ref re) = self.re {
            re.is_match(k)
        } else {
            self.m.contains(k)
        }
    }
}

/// Fetch the permit lists for the provided chemistry (or the chemistries matching the provided
/// regex) in the registry.
pub fn fetch_chemistries(
    af_home: PathBuf,
    fetch_opts: crate::simpleaf_commands::ChemistryFetchOpts,
) -> Result<()> {
    let dry_run_str = if fetch_opts.dry_run {
        "[dry_run] : "
    } else {
        ""
    };

    // check if the chemistry file is absent altogether
    // if so, then download it
    let chem_path = af_home.join(CHEMISTRIES_PATH);
    if !chem_path.is_file() {
        warn!(
            "The chemistry file is missing from {}; Nothing to download. To fetch the base chemistry registry itself, please issue the `refresh` command.",
            chem_path.display()
        );
    }

    let plist_path = af_home.join("plist");
    create_dir_if_absent(&plist_path)?;

    if let Some(chem_obj) = parse_resource_json_file(&chem_path, None)?.as_object() {
        // if the user used the special `*`, then we lookup all chemistries
        let fetch_chems: FetchSet = if fetch_opts.name.len() == 1 {
            FetchSet::from_re(fetch_opts.name.first().expect("First entry is valid"))?
        } else {
            // otherwise, collect just the set they requested
            let hs = HashSet::from_iter(fetch_opts.name.iter());
            FetchSet::from_hash_set(hs)
        };

        for (k, v) in chem_obj.iter() {
            // if we want to fetch this chem
            if fetch_chems.contains(k) {
                if let Some(serde_json::Value::String(pfile)) = v.get(LOCAL_PL_PATH_KEY) {
                    let fpath = plist_path.join(pfile);

                    // if it doesn't exist
                    if !fpath.is_file() {
                        //check for a remote path
                        if let Some(serde_json::Value::String(rpath)) = v.get(REMOTE_PL_URL_KEY) {
                            if fetch_opts.dry_run {
                                info!(
                                    "[dry_run] : Fetch would fetch missing file {} for {} from {}",
                                    pfile, k, rpath
                                );
                            } else {
                                let hash = download_to_file_compute_hash(rpath, &fpath)?;
                                let expected_hash = pfile.to_string();
                                let observed_hash = hash.to_string();
                                if expected_hash != observed_hash {
                                    warn!("Downloaded the file for chemistry {} from {}, but the observed hash {} was not equal to the expcted hash {}",
                                    k, rpath, observed_hash, expected_hash);
                                }
                                info!("Fetched permit list file for {} to {}", k, fpath.display());
                            }
                        } else {
                            warn!(
                                "{}Requested to obtain chemistry {}, but it has no remote URL!",
                                dry_run_str, k
                            );
                        }
                    } else {
                        info!(
                            "{}File for requested chemistry {} already exists ({}).",
                            dry_run_str,
                            k,
                            fpath.display()
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        add_chemistry, clean_chemistries, merge_deprecated_registry_entries,
        merge_registry_entries, parse_chemistry_version, removable_permit_lists, remove_chemistry,
    };
    use crate::simpleaf_commands::{ChemistryAddOpts, ChemistryCleanOpts, ChemistryRemoveOpts};
    use crate::utils::constants::CHEMISTRIES_PATH;
    use serde_json::{json, Map, Value};
    use std::collections::HashSet;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    /// Write a JSON value to `<af_home>/chemistries.json` for registry tests.
    fn write_registry(af_home: &std::path::Path, value: &Value) {
        fs::write(
            af_home.join(CHEMISTRIES_PATH),
            serde_json::to_string_pretty(value).unwrap(),
        )
        .unwrap();
    }

    /// Read `<af_home>/chemistries.json` as JSON.
    fn read_registry(af_home: &std::path::Path) -> Value {
        serde_json::from_str(&fs::read_to_string(af_home.join(CHEMISTRIES_PATH)).unwrap()).unwrap()
    }

    #[test]
    fn removable_permit_list_set_only_contains_unused_files() {
        let used = HashSet::from([PathBuf::from("plist/a"), PathBuf::from("plist/b")]);
        let present = HashSet::from([
            PathBuf::from("plist/a"),
            PathBuf::from("plist/b"),
            PathBuf::from("plist/c"),
        ]);

        let removable = removable_permit_lists(&used, &present);
        assert_eq!(removable, HashSet::from([PathBuf::from("plist/c")]));
    }

    #[test]
    fn merge_registry_entries_prefers_newer_versions_unless_forced() {
        let mut existing = Map::new();
        existing.insert(
            "chem_a".to_string(),
            json!({"geometry":"1{b[16]u[12]x:}2{r:}","expected_ori":"both","version":"1.0.0"}),
        );
        let mut incoming = Map::new();
        incoming.insert(
            "chem_a".to_string(),
            json!({"geometry":"1{b[16]u[10]x:}2{r:}","expected_ori":"both","version":"0.9.0"}),
        );
        incoming.insert(
            "chem_b".to_string(),
            json!({"geometry":"1{b[16]u[12]x:}2{r[50]x:}","expected_ori":"both","version":"0.1.0"}),
        );

        merge_registry_entries(&mut existing, &incoming, false, "").unwrap();
        assert_eq!(existing["chem_a"]["version"], json!("1.0.0"));
        assert_eq!(existing["chem_b"]["version"], json!("0.1.0"));

        merge_registry_entries(&mut existing, &incoming, true, "").unwrap();
        assert_eq!(existing["chem_a"]["version"], json!("0.9.0"));
    }

    #[test]
    fn merge_deprecated_registry_entries_inserts_only_valid_missing_entries() {
        let mut existing = Map::new();
        existing.insert(
            "already".to_string(),
            json!({"geometry":"1{b[16]u[12]x:}2{r:}","expected_ori":"both","version":"1.0.0"}),
        );
        let deprecated = Map::from_iter([
            (
                "already".to_string(),
                Value::String("1{b[10]}2{r:}".to_string()),
            ),
            (
                "valid_new".to_string(),
                Value::String("1{b[16]u[12]x:}2{r:}".to_string()),
            ),
            (
                "invalid_new".to_string(),
                Value::String("bad-geom".to_string()),
            ),
        ]);

        merge_deprecated_registry_entries(&mut existing, &deprecated, "");

        assert!(existing.contains_key("already"));
        assert!(existing.contains_key("valid_new"));
        assert!(!existing.contains_key("invalid_new"));
    }

    #[test]
    fn add_chemistry_only_updates_when_version_increases() {
        let tmp = tempdir().unwrap();
        write_registry(
            tmp.path(),
            &json!({
                "mychem": {
                    "geometry": "1{b[16]u[12]x:}2{r:}",
                    "expected_ori": "both",
                    "version": "1.2.0"
                }
            }),
        );

        add_chemistry(
            tmp.path().to_path_buf(),
            ChemistryAddOpts {
                name: "mychem".to_string(),
                geometry: Some("1{b[16]u[10]x:}2{r:}".to_string()),
                expected_ori: Some("both".to_string()),
                local_url: None,
                remote_url: None,
                version: Some("1.1.0".to_string()),
                from_json: None,
            },
        )
        .unwrap();
        let registry_after_older = read_registry(tmp.path());
        assert_eq!(registry_after_older["mychem"]["version"], json!("1.2.0"));
        assert_eq!(
            registry_after_older["mychem"]["geometry"],
            json!("1{b[16]u[12]x:}2{r:}")
        );

        add_chemistry(
            tmp.path().to_path_buf(),
            ChemistryAddOpts {
                name: "mychem".to_string(),
                geometry: Some("1{b[16]u[10]x:}2{r:}".to_string()),
                expected_ori: Some("both".to_string()),
                local_url: None,
                remote_url: None,
                version: Some("1.3.0".to_string()),
                from_json: None,
            },
        )
        .unwrap();
        let registry_after_newer = read_registry(tmp.path());
        assert_eq!(registry_after_newer["mychem"]["version"], json!("1.3.0"));
        assert_eq!(
            registry_after_newer["mychem"]["geometry"],
            json!("1{b[16]u[10]x:}2{r:}")
        );
    }

    #[test]
    fn remove_chemistry_respects_dry_run_and_regex_removal() {
        let tmp = tempdir().unwrap();
        write_registry(
            tmp.path(),
            &json!({
                "foo_chem": {"geometry":"1{b[16]u[12]x:}2{r:}","expected_ori":"both","version":"0.1.0"},
                "bar_chem": {"geometry":"1{b[16]u[12]x:}2{r:}","expected_ori":"both","version":"0.1.0"}
            }),
        );

        remove_chemistry(
            tmp.path().to_path_buf(),
            ChemistryRemoveOpts {
                name: "foo_.*".to_string(),
                dry_run: true,
            },
        )
        .unwrap();
        let after_dry_run = read_registry(tmp.path());
        assert!(after_dry_run.get("foo_chem").is_some());
        assert!(after_dry_run.get("bar_chem").is_some());

        remove_chemistry(
            tmp.path().to_path_buf(),
            ChemistryRemoveOpts {
                name: "foo_.*".to_string(),
                dry_run: false,
            },
        )
        .unwrap();
        let after_remove = read_registry(tmp.path());
        assert!(after_remove.get("foo_chem").is_none());
        assert!(after_remove.get("bar_chem").is_some());
    }

    #[test]
    fn clean_chemistries_dry_run_is_non_destructive_then_removes_unused() {
        let tmp = tempdir().unwrap();
        write_registry(
            tmp.path(),
            &json!({
                "chem": {
                    "geometry":"1{b[16]u[12]x:}2{r:}",
                    "expected_ori":"both",
                    "version":"0.1.0",
                    "plist_name":"keep_pl"
                }
            }),
        );

        let plist_dir = tmp.path().join("plist");
        fs::create_dir_all(&plist_dir).unwrap();
        let keep = plist_dir.join("keep_pl");
        let remove = plist_dir.join("remove_pl");
        fs::write(&keep, "keep").unwrap();
        fs::write(&remove, "remove").unwrap();

        clean_chemistries(
            tmp.path().to_path_buf(),
            ChemistryCleanOpts { dry_run: true },
        )
        .unwrap();
        assert!(keep.exists());
        assert!(remove.exists());

        clean_chemistries(
            tmp.path().to_path_buf(),
            ChemistryCleanOpts { dry_run: false },
        )
        .unwrap();
        assert!(keep.exists());
        assert!(!remove.exists());
    }

    #[test]
    fn parse_chemistry_version_rejects_invalid_versions() {
        let err = parse_chemistry_version("not-a-version", "unit test").unwrap_err();
        assert!(
            format!("{:#}", err).contains("Could not parse version"),
            "unexpected error: {:#}",
            err
        );
    }
}
