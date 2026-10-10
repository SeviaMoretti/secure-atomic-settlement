"""Identity checks and honest E7 coverage, independent of executor PASS labels."""
from __future__ import annotations

import hashlib
import json
import platform
import subprocess
from pathlib import Path

ROOT = Path('/home/gz785/code/exp2')
CONFIG = ROOT / 'configs/tdsc_v1/e7-v1.json'


def sha(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b''):
            digest.update(block)
    return digest.hexdigest()


def load(path):
    return json.loads(Path(path).read_text())


def matrix(config, repetitions):
    return [dict(baseline=b, operation=o, variant=v, repetition=r)
            for b in config['baselines'] for o in config['operations']
            for v in [*config['variants'], config['quota_variant']]
            if v != 'Q1' or o == 'IssueRight'
            for r in range(1, repetitions + 1)]


def identity(baselines):
    config = load(CONFIG)
    final_path = Path(config['source_final_manifest'])
    if sha(final_path) != config['source_final_manifest_sha256']:
        raise ValueError('source final manifest hash differs')
    final = load(final_path)
    candidate_path = Path(final['candidate']['path'])
    if sha(candidate_path) != final['candidate']['sha256']:
        raise ValueError('source candidate manifest hash differs')
    candidate = load(candidate_path)
    repos = {}
    for baseline in baselines:
        frozen = candidate['baselines'][baseline]
        root = frozen['worktree']
        head = subprocess.check_output(['git', '-C', root, 'rev-parse', 'HEAD'], text=True).strip()
        dirty = subprocess.check_output(['git', '-C', root, 'status', '--porcelain'], text=True).strip()
        if head != final['frozen_identity']['baseline_commits'][baseline] or dirty:
            raise ValueError(f'{baseline}: source identity or cleanliness mismatch')
        artifacts = []
        for artifact in frozen['executed_build_artifacts']:
            actual = sha(artifact['path'])
            if actual != artifact['sha256']:
                raise ValueError(f'{baseline}: artifact mismatch: {artifact["path"]}')
            artifacts.append({**artifact, 'observed_sha256': actual})
        repos[baseline] = dict(worktree=root, head=head, dirty=dirty, artifacts=artifacts)
    return dict(final_manifest=str(final_path), final_sha256=sha(final_path),
                candidate_manifest=str(candidate_path), candidate_sha256=sha(candidate_path),
                repositories=repos, kernel=platform.release(), machine=platform.machine(),
                config_sha256=sha(CONFIG))


BLOCKERS = {
    'V1': 'Lifecycle stale-state evidence is implemented provisionally; formal repetitions and all operation/baseline cells remain unrun.',
    'V2': 'Signed-field replacement with signature retention is implemented provisionally; the registered malicious-market contract artifact remains pending.',
    'V3': 'Four overlapping submissions and winner/loser evidence are implemented provisionally; formal repetitions remain unrun.',
    'V4': 'Composite field-replacement probe plus four-way overlap is implemented provisionally; the registered malicious-market contract artifact remains pending.',
    'Q1': 'Two overlapping last-quota candidates and winner settlement are implemented provisionally; formal repetitions remain unrun.',
}


def audit():
    config = load(CONFIG)
    return dict(schema_id='assetxchain-e7-readiness', schema_version=1,
                identity=identity(config['baselines']),
                pilot_units=len(matrix(config, config['pilot_repetitions'])),
                formal_units=len(matrix(config, config['formal_repetitions'])),
                implemented_scope='S0/S1 provisional E7 executor and checker coverage; not paper-final',
                e7_ready=False, blockers=BLOCKERS,
                additional_blockers=['B1/G3 malicious-market variants and their ABI/Wasm identity are absent.',
                                     'Same-block loser snapshots use an explicit boundary exemption; per-extrinsic isolation is represented by dispatch/concurrency evidence, not a separate state root.'])


if __name__ == '__main__':
    print(json.dumps(audit(), indent=2))
