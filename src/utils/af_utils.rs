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
            cmd.arg("--force-cells").arg(format!("{}", nc));
        }
        CellFilterMethod::ExpectCells(nc) => {
            cmd.arg("--expect-cells").arg(format!("{}", nc));
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

impl Chemistry {
    pub fn as_str(&self) -> &str {
        match self {
            Chemistry::TenxV2 => "10xv2",
            Chemistry::TenxV3 => "10xv3",
            Chemistry::Other(s) => s.as_str(),
        }
    }
}

pub enum PermitListResult {
    DownloadSuccessful(PathBuf),
    AlreadyPresent(PathBuf),
    UnregisteredChemistry,
}

pub struct CustomGeometry {
    barcode_desc: String,
    umi_desc: String,
    read_desc: String,
}

impl CustomGeometry {
    fn add_to_args(&self, cmd: &mut std::process::Command) {
        cmd.arg("--bc-geometry");
        cmd.arg(&self.barcode_desc);

        cmd.arg("--umi-geometry");
        cmd.arg(&self.umi_desc);

        cmd.arg("--read-geometry");
        cmd.arg(&self.read_desc);
    }
}

pub fn extract_geometry(geo: &str) -> Result<CustomGeometry> {
    if geo.contains(';') {
        // parse this as a custom
        let v: Vec<&str> = geo.split(';').collect();
        // one string must start with 'B', one with 'U' and one with 'R'
        if v.len() != 3 {
            return Err(anyhow!(
                "custom geometry should have 3 components (R,U,B), but {} were found",
                v.len()
            ));
        }

        let mut b_desc: Option<&str> = None;
        let mut u_desc: Option<&str> = None;
        let mut r_desc: Option<&str> = None;

        for e in v {
            let (t, ar) = e.split_at(1);
            match t {
                "B" => {
                    if let Some(prev_desc) = b_desc {
                        return Err(anyhow!("A description of the barcode geometry seems to appear > 1 time; previous desc = {}!", prev_desc));
                    }
                    b_desc = Some(ar);
                }
                "U" => {
                    if let Some(prev_desc) = u_desc {
                        return Err(anyhow!("A description of the umi geometry seems to appear > 1 time; previous desc = {}!", prev_desc));
                    }
                    u_desc = Some(ar);
                }
                "R" => {
                    if let Some(prev_desc) = r_desc {
                        return Err(anyhow!("A description of the read geometry seems to appear > 1 time; previous desc = {}!", prev_desc));
                    }
                    r_desc = Some(ar);
                }
                _ => {
                    return Err(anyhow!("Could not parse custom geometry, found descriptor type {}, but it must be of type (R,U,B)", t));
                }
            }
        }

        if let (Some(barcode_desc), Some(umi_desc), Some(read_desc)) = (&b_desc, &u_desc, &r_desc) {
            return Ok(CustomGeometry {
                barcode_desc: barcode_desc.to_string(),
                umi_desc: umi_desc.to_string(),
                read_desc: read_desc.to_string(),
            });
        } else {
            return Err(anyhow!(
                "Require B, U and R components: Status B ({:?}), U ({:?}), R ({:?})",
                b_desc,
                u_desc,
                r_desc
            ));
        }
    }
    Err(anyhow!(
        "custom geometry string doesn't contain ';' character"
    ))
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
        }
        None => {
            let custom_geo = extract_geometry(chem_str)?;
            custom_geo.add_to_args(cmd);
        }
    }
    Ok(())
}

pub fn get_permit_if_absent(af_home: &Path, chem: &Chemistry) -> Result<PermitListResult> {
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
