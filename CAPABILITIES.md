# Claw Code — Verified Capabilities

This repo's piece of the [ecosystem](../Void-Data-Compressor/CAPABILITIES.md).

Last verified: 2026-04-30, branch `claude/audit-remembrance-ecosystem-xaaUr`.

---

## Role in ecosystem

Rust port + tooling layer. Recently added to the substrate; this
audit's investigation surfaced that 5 hardcoded `REPO_ROOTS`
mappings across the void's scoring scripts were missing `clawcode`,
which silently failed all 100 of this repo's records to `unmeasured`
/ REJECT before the fix.

By function count: 100 functions across 64 PULL / 36 REFINE / 0 REJECT
(64% recognized — strong showing for a recently-added repo).
Per-repo modulator: μ=0.9259, modulator=1.0691 — **highest** in the
ecosystem, well above mean. claw-code's median coherency carries
the most lift in the per-repo modulator layer.

---

## ✅ Verified

| # | Capability | Test |
|---|---|---|
| 1 | All 100 functions scored under v3 (post-clawcode-fix) | `python3 -c "import json; d=json.load(open('../Void-Data-Compressor/cross_repo_function_records.json')); print(sum(1 for r in d['records'] if r['repo']=='clawcode'))"` returns 100 |
| 2 | Decision distribution | 64 PULL / 36 REFINE per `pipeline/decisions_summary.json::per_repo.clawcode` |
| 3 | Highest per-repo modulator | `pipeline/repo_modulators.json::modulators.clawcode.modulator` ≈ 1.0691 |
| 4 | Source paths resolve (post-fix) | `REPO_ROOTS['clawcode'] = '/home/user/claw-code'` now consistent across all 6 void-side scripts |

### Audit finding

When this repo was first added to `cross_repo_introspect.py`,
the corresponding `REPO_ROOTS` entries in 5 other void-side scripts
were not updated:

- `score_cross_repo_records.py` (both fast + slow paths)
- `compressor_service.py::_read_source()`
- `label_all_patterns.py`
- `merge_cross_repo_to_store.py`
- `stamp_ledger_from_git.py`

Each silently returned empty strings on file reads, producing 0
waveform/text/atomic scores → routed to REJECT as `cls=unmeasured`.
The bug surfaced through gate-distribution skew (76.7% REJECT
when only 8.4% expected) and was fixed in commit `2bb0931`.

The structural smell — same repo list duplicated across six files —
remains. Adding the next repo will trip the same class of bug
unless the list is centralised. See `CAPABILITIES.md` in
Void-Data-Compressor for the recommended `repo_paths.py` extraction.

---

## ❌ Out of scope here

- Substrate / scoring math — void
- Atomic table / covenant — oracle
- Pattern publication — blockchain

---

## Quick verification

```bash
cd ../Void-Data-Compressor
python3 -c "
import json
d = json.load(open('cross_repo_function_records.json'))
cc = [r for r in d['records'] if r['repo'] == 'clawcode']
print(f'clawcode records: {len(cc)}')
from collections import Counter
print('decisions:', Counter(r.get('gate_decision') for r in cc))
import statistics
unifieds = [r['coherency_v3']['unified'] for r in cc if r.get('coherency_v3')]
print(f'mean unified: {statistics.mean(unifieds):.4f}  (highest in ecosystem)')

# Modulator
rm = json.load(open('pipeline/repo_modulators.json'))
print(f'modulator: {rm[\"modulators\"][\"clawcode\"]}')
"
```

---

*Cross-cutting capabilities: see [`Void-Data-Compressor/CAPABILITIES.md`](../Void-Data-Compressor/CAPABILITIES.md).*
