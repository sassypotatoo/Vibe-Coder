#!/usr/bin/env python3
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def die(code: str) -> None:
    raise SystemExit('test_part34_10_18_alpha_node_publication_gate: ' + code)


alpha_workflow = (ROOT / '.github/workflows/android-diagnostic-apk.yml').read_text()
node_workflow = (ROOT / '.github/workflows/node-runtime-proof.yml').read_text()

for token in (
    'node-runtime-ready:',
    'name: Node runtime publication gate',
    'id: node_runtime_probe',
    'ready: ${{ steps.node_runtime_probe.outputs.ready }}',
    'ready=false',
    'Node runtime is not public yet; Alpha packaging will be skipped in this run.',
    'echo "ready=$ready" >> "$GITHUB_OUTPUT"',
    'needs: [jcode-android-proof-build, node-runtime-ready]',
    "if: ${{ needs.node-runtime-ready.outputs.ready == 'true' }}",
):
    if token not in alpha_workflow:
        die('alpha_gate_missing:' + token)

if 'Require public Node runtime before publishing Alpha' in alpha_workflow:
    die('racy_failing_alpha_preflight_still_present')

# Missing runtime must be a successful gate result, not a shell failure.
probe_start = alpha_workflow.find('  node-runtime-ready:')
alpha_start = alpha_workflow.find('  full-alpha-package:', probe_start)
if probe_start < 0 or alpha_start < 0:
    die('job_boundaries_missing')
probe = alpha_workflow[probe_start:alpha_start]
if 'exit 22' in probe or 'exit 1' in probe:
    die('publication_gate_may_fail_when_runtime_is_missing')
if 'if curl --fail' not in probe:
    die('publication_probe_not_conditionally_handled')

# The release workflow remains the authority for making the URL true and then
# explicitly starts a fresh Android run.
for token in (
    'Verify published Node runtime download URL',
    'Published Node runtime public URL verification PASSED',
    'Dispatch fresh Alpha after runtime is public',
    'gh workflow run android-diagnostic-apk.yml',
):
    if token not in node_workflow:
        die('post_publish_dispatch_missing:' + token)

print('Part 34.10.18 Alpha/Node publication-gate regression PASSED')
