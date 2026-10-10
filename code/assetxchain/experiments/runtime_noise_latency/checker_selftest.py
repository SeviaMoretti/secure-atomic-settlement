"""Mutation checks for the evidence gate. Never edits live raw results."""
import argparse,json,shutil,tempfile
from pathlib import Path
from analyze_controlled import analyze
def main():
 p=argparse.ArgumentParser();p.add_argument("smoke_run");args=p.parse_args();source=Path(args.smoke_run)
 mutations={
  "missing_noise":lambda d:d["noise"].pop(),
  "wrong_layout":lambda d:d["layout"].update(certificate_in_asset=True),
  "altered_target_hash":lambda d:d["target"].update(hash="0x"+"00"*32),
  "late_noise":lambda d:d["noise"][0].update(accepted_ms=d["target"]["sent_ms"]+1),
  "phase_slip":lambda d:d["target"].update(phase_ms=4500),
  "missing_transfer_event":lambda d:[b.update(events=[e for e in b["events"] if not(e["section"]=="dataAssets" and e["method"]=="CertificateTransferred")]) for b in d["blocks"]],
 }
 with tempfile.TemporaryDirectory(prefix="axc-noise-checker-") as temp:
  root=Path(temp)/"raw";shutil.copytree(source,root)
  assert analyze(root)["status"]=="PASS"
  file=root/"r00_dual_prefix_n512.json";original=file.read_text()
  for name,mutate in mutations.items():
   d=json.loads(original);mutate(d);file.write_text(json.dumps(d))
   try:analyze(root)
   except ValueError as e:print("REJECTED",name,str(e))
   else:raise AssertionError("accepted corruption: "+name)
   file.write_text(original)
 print("PASS: valid evidence accepted; six corruptions rejected")
if __name__=="__main__":main()
