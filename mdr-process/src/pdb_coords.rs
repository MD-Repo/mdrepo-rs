//! The x, y, z of a PDB ATOM/HETATM record (columns 31-54), read by one
//! rule for every reader in mdr-process. `sequence` and `output_checks`
//! stopped at a coordinate that did not parse; `psf_writer` and
//! `mmcif_writer` read it as 0.0 and wrote that position out
//! (mdrepo-rs#89, #112).
//!
//! Every coordinate must parse; one that does not is an error naming the
//! line and columns. A field of asterisks gets its own message: a writer
//! fills an 8.3f field that way when the value does not fit (below
//! -999.999 or above 9999.999 A), and such data is rejected, not read
//! (Travis, on #112), so the submitter is told which coordinate and why.
//!
//! Not detected: a C-style writer prints the wider number (`10000.500`)
//! instead of asterisks, which shifts every later column.

use anyhow::{Result, bail};

/// The three coordinate fields, as (start, end) byte columns
const FIELDS: [(usize, usize); 3] = [(30, 38), (38, 46), (46, 54)];

// --------------------------------------------------
/// One coordinate field, or an error naming its 1-based columns
fn field(line: &str, (a, b): (usize, usize)) -> Result<f64> {
    let text = line.get(a..b.min(line.len())).unwrap_or("").trim();
    if !text.is_empty() && text.chars().all(|c| c == '*') {
        bail!(
            "columns {}-{}: an overflowed coordinate ({text}): the value does \
             not fit the PDB format (-999.999 to 9999.999 A)",
            a + 1,
            b
        );
    }
    match text.parse::<f64>() {
        Ok(v) => Ok(v),
        Err(e) => bail!("columns {}-{}: {e}", a + 1, b),
    }
}

// --------------------------------------------------
/// The record's position. Callers add the line: see `read_xyz_at`.
pub fn read_xyz(line: &str) -> Result<[f64; 3]> {
    let [x, y, z] = FIELDS;
    Ok([field(line, x)?, field(line, y)?, field(line, z)?])
}

// --------------------------------------------------
/// `read_xyz` with the error in the form every reader reports:
/// `line N: columns A-B: <why>: "<the line>"` (`i` is 0-based)
pub fn read_xyz_at(i: usize, line: &str) -> Result<[f64; 3]> {
    read_xyz(line).map_err(|e| anyhow::anyhow!("line {}: {e}: {line:?}", i + 1))
}

// --------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn line(x: &str, y: &str, z: &str) -> String {
        format!(
            "ATOM      1  CA  ALA A   1    {x:>8}{y:>8}{z:>8}  1.00  0.00           C  "
        )
    }

    fn err(l: &str) -> String {
        read_xyz(l).unwrap_err().to_string()
    }

    #[test]
    fn a_plain_line_reads() {
        assert_eq!(
            read_xyz(&line("1.000", "-2.500", "3.250")).unwrap(),
            [1.0, -2.5, 3.25]
        );
    }

    #[test]
    fn the_extremes_that_fit_read() {
        assert_eq!(
            read_xyz(&line("-999.999", "0.000", "9999.999")).unwrap(),
            [-999.999, 0.0, 9999.999]
        );
    }

    #[test]
    fn an_overflow_on_any_axis_is_rejected_and_named() {
        for (x, y, z, cols) in [
            ("********", "0.0", "0.0", "31-38"),
            ("0.0", "********", "0.0", "39-46"),
            ("0.0", "0.0", "********", "47-54"),
        ] {
            let e = err(&line(x, y, z));
            assert!(
                e.starts_with(&format!("columns {cols}: an overflowed coordinate")),
                "{e}"
            );
            assert!(e.contains("-999.999 to 9999.999"), "{e}");
        }
    }

    #[test]
    fn the_first_bad_axis_is_the_one_named() {
        let e = err(&line("********", "********", "12.3ab"));
        assert!(e.starts_with("columns 31-38: an overflowed"), "{e}");
    }

    #[test]
    fn a_short_run_of_asterisks_is_an_overflow_too() {
        assert!(err(&line("****", "0.0", "0.0")).contains("overflowed"));
    }

    #[test]
    fn other_junk_is_an_error_naming_the_columns() {
        for z in ["12.3ab", "***.12", "", "1.2.3"] {
            let e = err(&line("0.0", "0.0", z));
            assert!(e.starts_with("columns 47-54: "), "{z:?}: {e}");
            assert!(!e.contains("overflowed"), "{z:?}: {e}");
        }
    }

    #[test]
    fn a_line_cut_short_is_an_error() {
        let l = line("1.0", "2.0", "3.0");
        assert!(read_xyz(&l[..46]).is_err());
        assert!(read_xyz(&l[..20]).is_err());
        assert!(read_xyz("").is_err());
    }

    #[test]
    fn the_line_from_issue_89_is_rejected() {
        // Verbatim from mdrepo-rs#89 (zenodo:13880099, line 132)
        let l = "ATOM     66  B     B    66       9.827 740.753********  1.00  \
                 0.00           B  ";
        let e = read_xyz_at(131, l).unwrap_err().to_string();
        assert!(
            e.starts_with("line 132: columns 47-54: an overflowed coordinate"),
            "{e}"
        );
        assert!(e.ends_with(&format!("{l:?}")), "{e}");
    }
}
