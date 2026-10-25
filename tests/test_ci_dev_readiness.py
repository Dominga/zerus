"""Dev build readiness is independent of reported regression failures."""
import os
from pathlib import Path
import re
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import test_ci_security as security
from test_ci_security import field, inline_python, job

ROOT = Path(__file__).resolve().parents[1]
WORKFLOWS = ROOT / '.github/workflows'


class DevReadiness(unittest.TestCase):
    def setUp(self):
        self.ci = (WORKFLOWS / 'ci.yml').read_text()
        self.build = (WORKFLOWS / 'ci-dev-qualification.yml').read_text()
        self.regression = (WORKFLOWS / 'ci-regression.yml').read_text()

    def test_main_dev_build_has_no_regression_dependency(self):
        arch = job(self.build, 'checks')
        self.assertEqual(field(arch, 'uses'), './.github/workflows/dev-arch.yml')
        self.assertNotIn('checks.yml', self.build)
        self.assertIn('run_tests: false', job(self.build, 'bundle'))
        self.assertNotIn('ci-regression.yml', self.ci + self.build)
        self.assertEqual(field(job(self.ci, 'dev-tail'), 'needs'), '[gate, qualification]')
        arch_build = (WORKFLOWS / 'dev-arch.yml').read_text()
        self.assertIn('ZERUS_RELEASE_CHANNEL=dev', arch_build)
        self.assertIn('scripts/build-arch-package.sh --build-only', arch_build)
        self.assertIn('scripts/check-installed-package.py', arch_build)

    def test_successful_packages_admit_dev_with_each_regression_result(self):
        code = compile(inline_python(job(self.build, 'gate')), 'Dev-build-gate', 'exec')
        for result in ('success', 'failure', 'cancelled', 'skipped'):
            with self.subTest(regression=result), patch.dict(os.environ, {
                    'CHECKS': 'success', 'BUNDLE': 'success', 'MACOS': 'true',
                    'REGRESSION': result}):
                exec(code, {})

    def test_missing_or_failed_builds_still_refuse_dev(self):
        code = compile(inline_python(job(self.build, 'gate')), 'Dev-build-gate', 'exec')
        for component in ('CHECKS', 'BUNDLE'):
            for result in ('failure', 'cancelled', 'skipped'):
                env = {'CHECKS': 'success', 'BUNDLE': 'success', 'MACOS': 'true'}
                env[component] = result
                with self.subTest(component=component, result=result), patch.dict(os.environ, env):
                    with self.assertRaises(SystemExit):
                        exec(code, {})

    def regression_selected(self, *, event='push', branch='main', repository='ufna/zerus',
                            source_repository='ufna/zerus', private=False,
                            enabled='true', conclusion='success', event_name='workflow_run'):
        expression = field(job(self.regression, 'checks'), 'if')
        expression = expression.replace('&&', ' and ').replace('||', ' or ')
        expression = re.sub(r'\bfalse\b', 'False', expression)
        return bool(eval(expression, {'__builtins__': {}}, {
            'github': SimpleNamespace(repository=repository, event_name=event_name, event=SimpleNamespace(
                repository=SimpleNamespace(private=private), workflow_run=SimpleNamespace(
                    event=event, head_branch=branch, conclusion=conclusion,
                    head_repository=SimpleNamespace(full_name=source_repository)))),
            'vars': SimpleNamespace(DEV_UPDATES_ENABLED=enabled),
        }))

    def test_regressions_run_after_success_or_failure_against_exact_build_source(self):
        for result in ('success', 'failure'):
            self.assertTrue(self.regression_selected(conclusion=result))
        self.assertFalse(self.regression_selected(conclusion='cancelled'))
        self.assertIn('workflows: [CI]', self.regression)
        self.assertIn('types: [completed]', self.regression)
        self.assertIn('ref: ${{ github.event.workflow_run.head_sha }}', self.regression)
        self.assertEqual(field(job(self.regression, 'checks'), 'uses'), './.github/workflows/checks.yml')
        self.assertNotIn('continue-on-error', self.regression)

    def test_other_sources_cannot_enter_the_main_regression_route(self):
        for context in (dict(event='pull_request'), dict(branch='topic'),
                        dict(repository='example/zerus'), dict(source_repository='example/zerus'),
                        dict(private=True), dict(enabled=''), dict(event_name='workflow_dispatch')):
            with self.subTest(context=context):
                self.assertFalse(self.regression_selected(**context))
        self.assertNotRegex(self.regression, r'(?m)^\s*(?:secrets:|environment:|contents: write|actions: write)')
        self.assertIn('cache-mode: read', self.regression)
        self.assertIn('save_caches: false', self.regression)

    def test_dev_package_remains_main_only_and_cache_writes_are_scoped(self):
        arch = (WORKFLOWS / 'dev-arch.yml').read_text()
        helper = security.WorkflowSecurityTests()
        self.assertTrue(helper.dev_selected(arch, 'arch-package', {}))
        for context in (dict(event='pull_request'), dict(event='workflow_run'),
                        dict(ref='refs/heads/topic'), dict(repository='example/zerus'),
                        dict(private=True), dict(enabled=''), dict(cancelled=True)):
            with self.subTest(context=context):
                self.assertFalse(helper.dev_selected(arch, 'arch-package', {}, **context))
        self.assertNotIn('secrets', arch)

    def test_publisher_and_sequence_keep_the_existing_ci_origin(self):
        publisher = (WORKFLOWS / 'publish-dev.yml').read_text()
        self.assertIn('workflows: [CI]', publisher)
        self.assertNotIn('CI regression', publisher)
        self.assertIn("WORKFLOW = '.github/workflows/ci.yml'", (ROOT / 'scripts/dev_updates.py').read_text())
        self.assertIn("github.run_number", (WORKFLOWS / 'dev-arch.yml').read_text())


if __name__ == '__main__':
    unittest.main()
