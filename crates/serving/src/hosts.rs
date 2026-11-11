use crate::{Result, ServingError as E};
use std::collections::{BTreeSet, HashSet};
/// Pure bounded patch preparation. Only the installation's exact marker block
/// changes; all bytes outside it, including other tools' comments, stay intact.
pub fn patch(before: &[u8], owner: &str, domains: &[String]) -> Result<Vec<u8>> {
    if !crate::site::owner(owner) || before.len() > 2 * 1024 * 1024 || domains.len() > 128 {
        return Err(E::InvalidConfiguration);
    }
    let text = std::str::from_utf8(before).map_err(|_| E::HostsConflict)?;
    let bom = if text.starts_with('\u{feff}') { 3 } else { 0 };
    let wanted: BTreeSet<_> = domains.iter().cloned().collect();
    for name in &wanted {
        crate::site::domain(name)?;
    }
    let begin = format!("# devforge:{owner} begin");
    let end = format!("# devforge:{owner} end");
    let mut inside = false;
    let mut range = None;
    let mut start = 0;
    let mut offset = bom;
    let mut prior = HashSet::new();
    for line in text[bom..].split_inclusive('\n') {
        let stripped = line.trim_end_matches(['\r', '\n']);
        if stripped == begin {
            if inside || range.is_some() {
                return Err(E::HostsConflict);
            }
            inside = true;
            start = offset;
        } else if stripped == end {
            if !inside {
                return Err(E::HostsConflict);
            }
            inside = false;
            range = Some((start, offset + line.len()));
        } else if inside {
            let body = stripped.split('#').next().unwrap().trim();
            if !body.is_empty() {
                let mut fields = body.split_whitespace();
                if fields.next() != Some("127.0.0.1") {
                    return Err(E::HostsConflict);
                }
                for name in fields {
                    crate::site::domain(name)?;
                }
            }
        } else {
            let mut fields = stripped.split('#').next().unwrap().split_whitespace();
            fields.next();
            for name in fields {
                prior.insert(name.to_ascii_lowercase());
            }
        }
        offset += line.len();
    }
    if inside || wanted.iter().any(|name| prior.contains(name)) {
        return Err(E::HostsConflict);
    }
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut block = String::new();
    if !wanted.is_empty() {
        block.push_str(&begin);
        block.push_str(newline);
        for name in wanted {
            block.push_str(&format!("127.0.0.1\t{name}{newline}"));
        }
        block.push_str(&end);
        block.push_str(newline);
    }
    let (start, end) = range.unwrap_or((bom, bom));
    Ok([
        &text.as_bytes()[..start],
        block.as_bytes(),
        &text.as_bytes()[end..],
    ]
    .concat())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changes_only_its_block_and_restores_original_bytes() {
        let owner = "a".repeat(32);
        let original = b"127.0.0.1 localhost\r\n# Herd marker keep\r\n10.0.0.1 foreign.test\r\n";
        let added = patch(original, &owner, &["demo.devforge.test".into()]).unwrap();
        assert!(added.ends_with(original));
        assert_eq!(patch(&added, &owner, &[]).unwrap(), original);
        assert_eq!(
            patch(&added, &owner, &["demo.devforge.test".into()]).unwrap(),
            added
        );
    }
    #[test]
    fn no_final_newline_and_utf8_bom_are_preserved() {
        let owner = "c".repeat(32);
        for original in [
            b"127.0.0.1 localhost".as_slice(),
            "\u{feff}127.0.0.1 localhost".as_bytes(),
        ] {
            let added = patch(original, &owner, &["a.devforge.test".into()]).unwrap();
            assert_eq!(patch(&added, &owner, &[]).unwrap(), original);
        }
    }
    #[test]
    fn never_adopts_foreign_entries_or_malformed_owned_blocks() {
        let owner = "b".repeat(32);
        assert_eq!(
            patch(
                b"127.0.0.1 demo.devforge.test\n",
                &owner,
                &["demo.devforge.test".into()]
            )
            .err(),
            Some(E::HostsConflict)
        );
        let bad = format!("# devforge:{owner} begin\n127.0.0.1 x.devforge.test\n");
        assert!(patch(bad.as_bytes(), &owner, &[]).is_err());
    }
}
