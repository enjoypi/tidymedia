use serde_derive::{Deserialize, Serialize};

use super::*;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Item(u32);

impl SpillKey for Item {
    fn spill_key(&self) -> String {
        format!("{:010}", self.0)
    }
}

fn collect(listed: &Listed<Item>) -> Vec<u32> {
    let mut out = Vec::new();
    listed
        .for_each(&mut |item| {
            out.push(item.0);
            Ok(())
        })
        .unwrap();
    out
}

#[test]
fn spilled_runs_merge_in_key_order() {
    let mut spill = Spill::default();
    spill.push_run(&[Item(5), Item(1), Item(9)]).unwrap();
    spill.push_run(&[]).unwrap();
    spill.push_run(&[Item(4), Item(2)]).unwrap();
    let listed = Listed::spilled(spill);
    assert!(listed.is_spilled());
    assert!(listed.is_empty());
    assert_eq!(collect(&listed), [1, 2, 4, 5, 9]);
    assert_eq!(serde_json::to_string(&listed).unwrap(), "[1,2,4,5,9]");
    assert_eq!(format!("{listed:?}"), "Spill(3 runs)");
}

#[test]
fn in_memory_items_keep_vec_behaviour() {
    let mut listed: Listed<Item> = vec![Item(3)].into();
    listed.push(Item(1));
    assert!(!listed.is_spilled());
    assert_eq!(collect(&listed), [3, 1]);
    assert_eq!(serde_json::to_string(&listed).unwrap(), "[3,1]");
    assert_eq!(format!("{listed:?}"), "[Item(3), Item(1)]");
    assert!(Listed::<Item>::default().is_empty());
}

#[test]
fn callback_errors_stop_the_merge() {
    let mut spill = Spill::default();
    spill.push_run(&[Item(1), Item(2)]).unwrap();
    let mut seen = 0;
    let err = spill
        .for_each(&mut |_| {
            seen += 1;
            Err(io::Error::other("stop"))
        })
        .unwrap_err();
    assert_eq!((seen, err.to_string().as_str()), (1, "stop"));
}

#[test]
fn corrupt_and_missing_runs_surface_as_errors() {
    for content in [
        &b"no tab\n"[..],
        b"not-json\t1\n",
        b"\"0\"\tnot-json\n",
        b"\xff\xfe\n",
    ] {
        let mut spill: Spill<Item> = Spill::default();
        spill.push_run(&[Item(1)]).unwrap();
        std::fs::write(&spill.runs.files[0], content).unwrap();
        assert!(spill.for_each(&mut |_| Ok(())).is_err());
        assert!(serde_json::to_string(&Listed::spilled(spill)).is_err());
    }
    let mut spill: Spill<Item> = Spill::default();
    spill.push_run(&[Item(1), Item(2)]).unwrap();
    let mut lines = std::fs::read_to_string(&spill.runs.files[0]).unwrap();
    lines.push_str("broken\n");
    std::fs::write(&spill.runs.files[0], lines).unwrap();
    assert!(spill.for_each(&mut |_| Ok(())).is_err());
    std::fs::remove_file(&spill.runs.files[0]).unwrap();
    assert!(spill.for_each(&mut |_| Ok(())).is_err());
}

#[test]
fn unwritable_spill_dir_fails_push() {
    let mut spill: Spill<Item> = Spill::in_dir(PathBuf::from("/no/such/spill/dir"));
    assert!(spill.push_run(&[Item(1)]).is_err());
    let redirected: Spill<Item> =
        with_spill_dir(PathBuf::from("/no/such/spill/dir"), Spill::default);
    assert_eq!(redirected.runs.dir, PathBuf::from("/no/such/spill/dir"));
}

struct FailingWriter(usize);

impl std::io::Write for FailingWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.0 < buf.len() {
            return Err(std::io::Error::other("full"));
        }
        self.0 -= buf.len();
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn serializer_errors_stop_streaming() {
    let mut spill = Spill::default();
    spill.push_run(&[Item(1), Item(2)]).unwrap();
    let listed = Listed::spilled(spill);
    assert!(serde_json::to_writer(FailingWriter(0), &listed).is_err());
    assert!(serde_json::to_writer(FailingWriter(2), &listed).is_err());
}
