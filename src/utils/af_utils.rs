use anyhow::{anyhow, Result};
use cmd_lib::run_fun;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::error;

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

#[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub struct GeomPiece {
    read_num: u32,
    pos_start: u32,
    length: u32,
    kind: char,
}

impl GeomPiece {
    fn append_piece_piscem_style(
        &self,
        gstr: &mut String,
        curr_read: &mut u32,
        last_pos: &mut u32,
    ) {
        // if we haven't started constructing gstr yet
        if gstr.is_empty() {
            // this is the first piece
            *gstr += &format!("{}{{", self.read_num);
        } else if self.read_num != *curr_read {
            // if this isn't the first piece
            if *last_pos == u32::MAX {
                *gstr += "}";
            } else {
                *gstr += "x:}";
            }
            *gstr += &format!("{}{{", self.read_num);
            *last_pos = 0;
        }
        *curr_read = self.read_num;

        // regardless of if this is the first piece or not
        // we have to add the geometry description
        let prefix_x = self.pos_start - (*last_pos + 1);
        if prefix_x > 0 {
            *gstr += &format!("x[{}]", prefix_x);
        }
        let l = self.length;
        if l < u32::MAX {
            *gstr += &format!("{}[{}]", self.kind, self.length);
            *last_pos += prefix_x + self.length;
        } else {
            *gstr += &format!("{}:", self.kind);
            *last_pos = u32::MAX;
        }
    }
}

// parses the range [x-y] from x to y into
// a pair of a staring offset and a length
// if the range is of the form [x-end] then
// we set y = u32::MAX.
fn parse_range(r: &str) -> (u32, u32) {
    let v: Vec<&str> = r.split('-').collect();
    if let (Some(s), Some(e)) = (v.first(), v.last()) {
        let s = s.parse::<u32>().unwrap();
        let e = if e == &"end" {
            u32::MAX
        } else {
            e.parse::<u32>().unwrap()
        };
        let l = if e < u32::MAX { (e - s) + 1 } else { u32::MAX };
        println!("range is (start : {}, len : {})", s, l);
        (s, l)
    } else {
        panic!("could not parse range {}", r);
    }
}

impl CustomGeometry {
    fn add_to_args_salmon(&self, cmd: &mut std::process::Command) {
        cmd.arg("--bc-geometry");
        cmd.arg(&self.barcode_desc);

        cmd.arg("--umi-geometry");
        cmd.arg(&self.umi_desc);

        cmd.arg("--read-geometry");
        cmd.arg(&self.read_desc);
    }

    fn add_to_args_piscem(&self, cmd: &mut std::process::Command) {
        // get the read information for each part
        let bread = match &self.barcode_desc.chars().next() {
            Some('1') => 1,
            Some('2') => 2,
            _ => {
                error!("invalid read specified for barcode location");
                panic!("invalid read specified for barcode location");
            }
        };
        let uread = match &self.umi_desc.chars().next() {
            Some('1') => 1,
            Some('2') => 2,
            _ => {
                error!("invalid read specified for UMI location");
                panic!("invalid read specified for UMI location");
            }
        };
        let rread = match &self.read_desc.chars().next() {
            Some('1') => 1,
            Some('2') => 2,
            _ => {
                error!("invalid read specified for biological sequence location");
                panic!("invalid read specified for biological sequence location");
            }
        };

        let brange = parse_range(&self.barcode_desc[2..(&self.barcode_desc.len() - 1)]);
        let urange = parse_range(&self.umi_desc[2..(&self.umi_desc.len() - 1)]);
        let rrange = parse_range(&self.read_desc[2..(&self.read_desc.len() - 1)]);

        let mut elements = vec![
            GeomPiece {
                read_num: bread,
                pos_start: brange.0,
                length: brange.1,
                kind: 'b',
            },
            GeomPiece {
                read_num: uread,
                pos_start: urange.0,
                length: urange.1,
                kind: 'u',
            },
            GeomPiece {
                read_num: rread,
                pos_start: rrange.0,
                length: rrange.1,
                kind: 'r',
            },
        ];
        elements.sort();

        let mut gstr = String::new();
        let mut curr_read = 0_u32;
        let mut last_pos = 0_u32;

        elements[0].append_piece_piscem_style(&mut gstr, &mut curr_read, &mut last_pos);
        elements[1].append_piece_piscem_style(&mut gstr, &mut curr_read, &mut last_pos);
        elements[2].append_piece_piscem_style(&mut gstr, &mut curr_read, &mut last_pos);
        if last_pos == u32::MAX {
            gstr += "}";
        } else {
            gstr += "x:}";
        }

        cmd.arg("--geometry").arg(gstr);
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

pub fn add_chemistry_to_args_salmon(chem_str: &str, cmd: &mut std::process::Command) -> Result<()> {
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
            custom_geo.add_to_args_salmon(cmd);
        }
    }
    Ok(())
}

pub fn add_chemistry_to_args_piscem(chem_str: &str, cmd: &mut std::process::Command) -> Result<()> {
    let known_chem_map = HashMap::from([("10xv2", "chromium_v2"), ("10xv3", "chromium_v3")]);

    match known_chem_map.get(chem_str) {
        Some(v) => {
            cmd.arg("--geometry").arg(v);
        }
        None => {
            let custom_geo = extract_geometry(chem_str)?;
            custom_geo.add_to_args_piscem(cmd);
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
