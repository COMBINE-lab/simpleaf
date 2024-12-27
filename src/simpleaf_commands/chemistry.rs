use crate::utils::af_utils::*;
use crate::utils::chem_utils::{
    custom_chem_hm_into_json, get_custom_chem_hm, get_single_custom_chem_from_file,
    CustomChemistry, LOCAL_PL_PATH_KEY, REMOTE_PL_URL_KEY,
};
use crate::utils::constants::*;
use crate::utils::prog_utils::{self, download_to_file_compute_hash};
use regex::Regex;

use anyhow::{bail, Context, Result};
use semver::Version;
use serde_json::json;
use std::collections::HashSet;
use std::fs;
use std::io::{Seek, Write};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use tracing::{info, warn};

/// Adds a chemsitry to the `chemistries.json` file. The user provides a name
/// and geometry string, and optionally a local-url and / or remote-url.  
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
    add_opts: crate::simpleaf_commands::ChemistryAddOpts,
) -> Result<()> {
    let geometry = add_opts.geometry;
    // check geometry string, if no good then
    // propagate error.
    validate_geometry(&geometry)?;

    let version = add_opts
        .version
        .unwrap_or(CustomChemistry::default_version());
    let add_ver = Version::parse(version.as_ref()).with_context(|| format!("could not parse version {}. Please follow https://semver.org/. A valid example is 0.1.0", version))?;

    let name = add_opts.name;

    // read in the custom chemistry file
    let chem_p = af_home_path.join(CHEMISTRIES_PATH);

    if let Some(existing_entry) = get_single_custom_chem_from_file(&chem_p, &name)? {
        let existing_ver_str = existing_entry.version();
        let existing_ver = Version::parse(existing_ver_str).with_context( || format!("could not parse version {} found in existing chemistries.json file. Please correct this entry", existing_ver_str))?;
        if add_ver <= existing_ver {
            info!("Attempting to add chemistry with version {:#} which is <= than the existing version ({:#}) for this chemistry. Skipping addition", add_ver, existing_ver);
            return Ok(());
        } else {
            info!(
                "Updating existing version {:#} of chemistry {} to {:#}",
                existing_ver, name, add_ver
            );
        }
    }

    let local_plist;
    if let Some(local_url) = add_opts.local_url {
        if local_url.is_file() {
            let metadata = std::fs::metadata(&local_url)?;
            let flen = metadata.size();
            if flen > 4_294_967_296 {
                warn!("The file provided to local-url ({}) is {} bytes. This file will be *copied* into the ALEVIN_FRY_HOME directory", local_url.display(), flen);
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
                info!("A content-equivalent permit list file already exists; will use the exising file.");
            } else {
                info!(
                    "Copying {} to {}",
                    local_url.display(),
                    local_plist_path.display()
                );
                std::fs::copy(&local_url, &local_plist_path).with_context(|| {
                    format!(
                        "failed to copy local permit list url {} to location {}",
                        local_url.display(),
                        local_plist_path.display()
                    )
                })?;
            }
            local_plist = Some(local_plist_name.display().to_string());
        } else {
            bail!("The local-url {} was provided, but no file could be found at that location. Cannot continue.", local_url.display());
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
                "A content-equivalent permit list file already exists; will use the exising file."
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

    // init the custom chemistry struct
    let custom_chem = CustomChemistry {
        name,
        geometry,
        expected_ori: ExpectedOri::from_str(&add_opts.expected_ori)?,
        plist_name: local_plist,
        remote_pl_url: add_opts.remote_url,
        version,
        meta: None,
    };

    let mut chem_hm = get_custom_chem_hm(&chem_p)?;

    // check if the chemistry already exists and log
    if let Some(cc) = chem_hm.get(custom_chem.name()) {
        info!("chemistry {} already existed, with geometry {} the one recorded: {}; overwriting geometry specification", custom_chem.name(), if cc.geometry() == custom_chem.geometry() {"same as"} else {"different with"}, cc.geometry());
        chem_hm
            .entry(custom_chem.name().to_string())
            .and_modify(|e| *e = custom_chem);
    } else {
        info!(
            "inserting chemistry {} with geometry {}",
            custom_chem.name(),
            custom_chem.geometry()
        );
        chem_hm.insert(custom_chem.name().to_string(), custom_chem);
    }

    // convert the custom chemistry hashmap to json
    let v = custom_chem_hm_into_json(chem_hm)?;

    // write out the new custom chemistry file
    let mut custom_chem_file = std::fs::File::create(&chem_p)
        .with_context(|| format!("could not create {}", chem_p.display()))?;
    custom_chem_file.rewind()?;

    custom_chem_file
        .write_all(serde_json::to_string_pretty(&v).unwrap().as_bytes())
        .with_context(|| format!("could not write {}", chem_p.display()))?;

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
    // if the old custom chem file exists, then warn the user about it
    // but read it in and attempt to populate.
    let custom_chem_file = af_home.join(CUSTOM_CHEMISTRIES_PATH);
    let merge_custom_chem = if custom_chem_file.exists() {
        warn!("The \"custom_chemistries.json\" file is deprecated, and in the future, these chemistries should be 
        regustered in the \"chemistries.json\" file instead. We will attempt to automatically migrate over the old 
        chemistries into the new file");
        true
    } else {
        false
    };

    // check if the chemistry file is absent altogether
    // if so, then download it
    let chem_path = af_home.join(CHEMISTRIES_PATH);
    if !chem_path.is_file() {
        prog_utils::download_to_file(CHEMISTRIES_URL, &chem_path)?;
    } else {
        let tmp_chem_path = af_home.join(CHEMISTRIES_PATH).with_extension("tmp.json");
        prog_utils::download_to_file(CHEMISTRIES_URL, &tmp_chem_path)?;
        if let Some(existing_chem) = parse_resource_json_file(&chem_path, None)?.as_object_mut() {
            if let Some(new_chem) = parse_resource_json_file(&tmp_chem_path, None)?.as_object() {
                for (k, v) in new_chem.iter() {
                    match existing_chem.get_mut(k) {
                        None => {
                            existing_chem.insert(k.clone(), v.clone());
                        }
                        Some(ev) => {
                            let curr_ver = Version::parse(
                                ev.get("version")
                                    .expect("chemistry should have a version field")
                                    .as_str()
                                    .expect("version should be a string"),
                            )?;
                            let new_ver = Version::parse(
                                v.get("version")
                                    .expect("chemistry should have a version field")
                                    .as_str()
                                    .expect("version should be a string"),
                            )?;
                            if refresh_opts.force || new_ver > curr_ver {
                                info!("updating {}", k);
                                existing_chem.insert(k.clone(), v.clone());
                            }
                        }
                    }
                }

                // write out the merged chemistry file
                let mut chem_file = std::fs::File::create(&chem_path)
                    .with_context(|| format!("could not create {}", chem_path.display()))?;
                chem_file.rewind()?;

                chem_file
                    .write_all(
                        serde_json::to_string_pretty(&existing_chem)
                            .unwrap()
                            .as_bytes(),
                    )
                    .with_context(|| format!("could not write {}", chem_path.display()))?;

                // remove the temp file
                std::fs::remove_file(tmp_chem_path)?;
            } else {
                bail!("Could not parse newly downloaded \"chemistries.json\" file as a JSON object, something is wrong. Please report this on GitHub.");
            }
        } else {
            bail!("Could not parse existing \"chemistries.json\" file as a JSON object, something is wrong. Please report this on GitHub.");
        }
    }

    if merge_custom_chem {
        if let Some(new_chem) = parse_resource_json_file(&chem_path, None)?.as_object_mut() {
            if let Some(old_custom_chem) =
                parse_resource_json_file(&custom_chem_file, None)?.as_object()
            {
                for (k, v) in old_custom_chem.iter() {
                    if new_chem.contains_key(k) {
                        warn!("The newly downloaded \"chemistries.json\" file already contained the key {}, skipping entry from the existing \"custom_chemistries.json\" file.", k);
                    } else {
                        let new_ent = json!({
                            "geometry": v,
                            "expected_ori": "both",
                            "version" : "0.1.0"
                        });
                        new_chem.insert(k.to_owned(), new_ent);
                        info!("successfully inserted {} from old custom chemistries file into the new chemistries registry", k);
                    }
                }

                // write out the merged chemistry file
                let mut chem_file = std::fs::File::create(&chem_path)
                    .with_context(|| format!("could not create {}", chem_path.display()))?;
                chem_file.rewind()?;

                chem_file
                    .write_all(serde_json::to_string_pretty(&new_chem).unwrap().as_bytes())
                    .with_context(|| format!("could not write {}", chem_path.display()))?;

                let backup = custom_chem_file.with_extension("json.bak");
                std::fs::rename(custom_chem_file, backup)?;
            } else {
                bail!("Could not parse existing \"custom_chemistries.json\" file as a JSON object; it may be corrupted. Consider deleting this file.");
            }
        } else {
            bail!("Could not parse newly downloaded \"chemistries.json\" file as a JSON object, something is wrong. Please report this on GitHub.");
        }
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
        return Ok(());
    }

    let chem_hm = get_custom_chem_hm(&chem_p)?;

    let used_pls = chem_hm
        .values()
        .filter_map(|v| v.plist_name().as_ref().map(|s| PathBuf::from(s.clone())))
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

    let rem_pls = &present_pls - &used_pls;
    // check if the chemistry already exists and log
    if dry_run {
        info!("The following files in the permit list directory are unused and would be removed: {:#?}", rem_pls);
    } else {
        for pl in rem_pls {
            info!("removing {}", pl.display());
            std::fs::remove_file(pl)?;
        }
    }

    Ok(())
}

/// Remove the entry for the provided chemistry in `chemistries.json` if it is present.
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
                        "[dry_run] : would remove chemistry {} from the registry.",
                        k
                    );
                } else {
                    info!("chemistry {} found in the registry; removing it!", k);
                    chem_hm.remove(&k);
                }
            }
        }
    } else {
        bail!(
            "The provided chemistry name {} was neither a valid chemistry name nor a valid regex",
            name
        );
    }

    if num_matched == 0 {
        info!(
            "no chemistry with name {} (or matching this as a regex) was found in the registry; nothing to remove",
            name
        );
    } else if !remove_opts.dry_run {
        // convert the custom chemistry hashmap to json
        let v = custom_chem_hm_into_json(chem_hm)?;

        // write out the new custom chemistry file
        let mut custom_chem_file = std::fs::File::create(&chem_p)
            .with_context(|| format!("could not create {}", chem_p.display()))?;
        custom_chem_file.rewind()?;

        custom_chem_file
            .write_all(serde_json::to_string_pretty(&v).unwrap().as_bytes())
            .with_context(|| format!("could not write {}", chem_p.display()))?;
    }

    Ok(())
}

pub fn lookup_chemistry(
    af_home_path: PathBuf,
    lookup_opts: crate::simpleaf_commands::ChemistryLookupOpts,
) -> Result<()> {
    let name = lookup_opts.name;
    // read in the custom chemistry file
    let chem_p = af_home_path.join(CHEMISTRIES_PATH);

    let chem_hm = get_custom_chem_hm(&chem_p)?;

    // check if the chemistry already exists and log
    if let Some(cc) = chem_hm.get(&name) {
        println!("chemistry name : {}", name);
        println!("==============");
        println!("{:#?}", cc);
    } else {
        info!("no chemistry with name {} was found in the registry!", name);
        info!(
            "treating {} as a regex and searching for matching chemistries",
            name
        );
        if let Ok(re) = Regex::new(&name) {
            for (cname, cval) in chem_hm.iter() {
                if re.is_match(cname) {
                    println!("chemistry name : {}", cname);
                    println!("==============");
                    println!("{:#?}", cval);
                }
            }
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
            bail!("could not compile regex : [{}]", s)
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

pub fn fetch_chemistries(
    af_home: PathBuf,
    refresh_opts: crate::simpleaf_commands::ChemistryFetchOpts,
) -> Result<()> {
    if refresh_opts.chemistries.is_empty() {
        bail!("The list of chemistries to fetch was empty; nothing to do!");
    }

    // check if the chemistry file is absent altogether
    // if so, then download it
    let chem_path = af_home.join(CHEMISTRIES_PATH);
    if !chem_path.is_file() {
        bail!(
            "The chemistry file was missing from {}; nothing to download.",
            chem_path.display()
        );
    }

    let plist_path = af_home.join("plist");
    create_dir_if_absent(&plist_path)?;

    if let Some(chem_obj) = parse_resource_json_file(&chem_path, None)?.as_object() {
        // if the user used the special `*`, then we lookup all chemistries
        let fetch_chems: FetchSet = if refresh_opts.chemistries.len() == 1 {
            FetchSet::from_re(
                refresh_opts
                    .chemistries
                    .first()
                    .expect("first entry is valid"),
            )?
        } else {
            // otherwise, collect just the set they requested
            let hs = HashSet::from_iter(refresh_opts.chemistries.iter());
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
                            if refresh_opts.dry_run {
                                info!(
                                    "fetch would fetch missing file {} for {} from {}",
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
                            }
                        } else {
                            warn!(
                                "requested to obtain chemistry {}, but it has no remote URL!",
                                k
                            );
                        }
                    } else {
                        info!(
                            "file for requested chemistry {} already exists ({}).",
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
