#!/usr/bin/env python3
"""Optional isolated rebuild. Does not alter the canonical source or target directory."""
import argparse,os,shutil,subprocess
from pathlib import Path
ROOT=Path("/home/gz785/code/assetxchain")
EXP=ROOT/"experiments/runtime_noise_latency"
BASE="cf3859eeadfc7286a1f0656a148a9fa245f82605"
def call(*argv,**kw):subprocess.run(argv,check=True,**kw)
def main():
 p=argparse.ArgumentParser();p.add_argument("--build",action="store_true");a=p.parse_args()
 for variant in ["dual_prefix","single_tree"]:
  tree=EXP/"worktrees"/variant
  if not tree.exists():
   tree.parent.mkdir(parents=True,exist_ok=True)
   call("git","-C",str(ROOT),"worktree","add","--detach",str(tree),BASE)
  head=subprocess.check_output(["git","-C",str(tree),"rev-parse","HEAD"],text=True).strip()
  if head!=BASE:raise RuntimeError("worktree HEAD mismatch; preserved existing worktree")
  expected=(EXP/"single-tree.patch").read_text() if variant=="single_tree" else ""
  actual=subprocess.check_output(["git","-C",str(tree),"diff","--"],text=True)
  if variant=="single_tree" and not actual:
   call("git","-C",str(tree),"apply","--check",str(EXP/"single-tree.patch"))
   call("git","-C",str(tree),"apply",str(EXP/"single-tree.patch"))
   actual=subprocess.check_output(["git","-C",str(tree),"diff","--"],text=True)
  if actual!=expected:raise RuntimeError("unrelated worktree diff; preserved worktree")
  if a.build:
   env=os.environ.copy();env["CARGO_TARGET_DIR"]=str(EXP/"build"/variant)
   call("cargo","build","--locked","--release","-p","assetxchain-node","-j","2",cwd=tree,env=env)
   (EXP/"artifacts").mkdir(exist_ok=True)
   shutil.copy2(EXP/"build"/variant/"release/assetxchain-node",EXP/"artifacts"/("node-"+variant))
  print(variant,tree,"built" if a.build else "prepared")
if __name__=="__main__":main()
