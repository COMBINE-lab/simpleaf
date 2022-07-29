use anyhow::{anyhow, Result};
use cmd_lib::run_fun;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub enum CellFilterMethod {
    // cut off at this cell in
    // the frequency sorted list
    ForceCells(usize),
    // use this cell as a hint in
    // the frequency sorted list
    ExpectCells(usize),
    // correct all cells in an
    // edit distance of 1 of these
    // barcodes
    ExplicitList(String),
    // barcodes will be provided in the
    // form of an *unfiltered* external
    // permit list
    UnfilteredExternalList(String, usize),
    // use the distance method to
    // automatically find the knee
    // in the curve
    KneeFinding,
}

pub fn add_to_args(fm: &CellFilterMethod, cmd: &mut std::process::Command) {
    match fm {
        CellFilterMethod::ForceCells(nc) => {
            cmd.arg("--force").arg(format!("{}", nc));
        }
        CellFilterMethod::ExpectCells(nc) => {
            cmd.arg("--force").arg(format!("{}", nc));
        }
        CellFilterMethod::ExplicitList(l) => {
            cmd.arg("--valid-bc").arg(l);
        }
        CellFilterMethod::UnfilteredExternalList(l, m) => {
            cmd.arg("--unfiltered-pl")
                .arg(l)
                .arg("--min-reads")
                .arg(format!("{}", m));
        }
        CellFilterMethod::KneeFinding => {
            cmd.arg("--knee-distance");
        }
    }
}

pub enum Chemistry {
    TenxV2,
    TenxV3,
    Other(String),
}

pub enum PermitListResult {
    DownloadSuccessful(PathBuf),
    AlreadyPresent(PathBuf),
    UnregisteredChemistry,
}

pub fn add_chemistry_to_args(chem_str: &str, cmd: &mut std::process::Command) -> Result<()> {
    let known_chem_map = HashMap::from([
        ("10xv2", "--chromium"),
        ("10xv3", "--chromiumV3"),
        ("dropseq", "--dropseq"),
        ("indropv2", "--indropV2"),
        ("citeseq", "--citeseq"),
        ("gemcode", "--gemcode"),
        ("celseq", "--celseq"),
        ("celseq2", "--celseq2"),
        ("splitseqv1", "--splitseqV1"),
        ("splitseqv2", "--splitseqV2"),
        ("sciseq3", "--sciseq3"),
    ]);

    match known_chem_map.get(chem_str) {
        Some(v) => {
            cmd.arg(v);
            return Ok(());
        }
        None => {
            if chem_str.contains(';') {
                // parse this as a custom
                let v: Vec<&str> = chem_str.split(';').collect();
                // one string must start with 'B', one with 'U' and one with 'R'
                if v.len() != 3 {
                    return Err(anyhow!(
                        "custom geometry should have 3 components (R,U,B), but {} were found",
                        v.len()
                    ));
                }
                for e in v {
                    let (t, ar) = e.split_at(1);
                    match t {
                        "B" => {
                            cmd.arg("--bc-geometry");
                            cmd.arg(ar);
                        }
                        "U" => {
                            cmd.arg("--umi-geometry");
                            cmd.arg(ar);
                        }
                        "R" => {
                            cmd.arg("--read-geometry");
                            cmd.arg(ar);
                        }
                        _ => {
                            return Err(anyhow!("Could not parse custom geometry, found descriptor type {}, but it must be of type (R,U,B)", t));
                        }
                    }
                }
                return Ok(());
            }
        }
    }

    Err(anyhow!(
        "Could not recognize {} as either a known or custom chemistry!",
        chem_str
    ))
}

pub fn get_permit_if_absent(af_home: &Path, chem: Chemistry) -> Result<PermitListResult> {
    let chem_file;
    let dl_url;
    match chem {
        Chemistry::TenxV2 => {
            chem_file = "10x_v2_permit.txt";
            dl_url = "https://umd.box.com/shared/static/jbs2wszgbj7k4ic2hass9ts6nhqkwq1p";
        }
        Chemistry::TenxV3 => {
            chem_file = "10x_v3_permit.txt";
            dl_url = "https://umd.box.com/shared/static/eo0qlkfqf2v24ws6dfnxty6gqk1otf2h";
        }
        _ => {
            return Ok(PermitListResult::UnregisteredChemistry);
        }
    }

    let odir = af_home.join("plist");
    if odir.join(chem_file).exists() {
        Ok(PermitListResult::AlreadyPresent(odir.join(chem_file)))
    } else {
        run_fun!(mkdir -p $odir)?;
        let mut dl_cmd = std::process::Command::new("wget");
        dl_cmd
            .arg("-v")
            .arg("-O")
            .arg(odir.join(chem_file).to_string_lossy().to_string())
            .arg("-L")
            .arg(dl_url);
        let r = dl_cmd.output()?;
        if !r.status.success() {
            return Err(anyhow!("failed to download permit list {:?}", r.status));
        }
        Ok(PermitListResult::DownloadSuccessful(odir.join(chem_file)))
    }
}
