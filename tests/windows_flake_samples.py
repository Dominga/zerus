"""Opt-in loaded Windows qualification, with no retry after an uncertain choice."""
from contextlib import contextmanager
import hashlib
import json
import os
import re
import sys
import threading
import unittest


def round_count(environment=None):
    value = (os.environ if environment is None else environment).get('ZERUS_WINDOWS_FLAKE_ROUNDS', '0')
    if not re.fullmatch(r'0|[1-9][0-9]?', value) or int(value) > 40:
        raise ValueError('ZERUS_WINDOWS_FLAKE_ROUNDS must be an integer from 0 to 40')
    return int(value)


def mode_rounds(rounds, grant, choose):
    assert type(rounds) is int and 0 <= rounds <= 40
    seen = set()
    for index in range(rounds):
        # A successful passive rollback revokes ownership. Each later round
        # is a fresh fixture choice and must explicitly Rearm that history.
        grant('ownership_grant' if index == 0 else 'ownership_rearm')
        for desired in ('background', 'passive'):
            value = choose(desired)
            print('MODE_SAMPLE_CHOICE ' + json.dumps(dict(round=index+1, desired=desired, result=value)),
                  file=sys.stderr)
            assert value.get('ok') is True, json.dumps(value)
            assert value.get('running_mode') == desired, json.dumps(value)
            assert value.get('changed') is True, 'only a changed mode qualifies a Stop sample'
            timing = value.get('stop_timing')
            assert isinstance(timing, dict) and timing.get('abi') == 'desktop-mode-stop-timing-v1', 'missing Stop timing'
            assert timing.get('budget_ms') == 4000, 'Stop timing must report the approved four-second budget'
            for name in ('native_rechecks', 'metadata_sync', 'instance_stop', 'physical_exit'):
                assert isinstance(timing.get('steps', {}).get(name, {}).get('elapsed_ms'), int), 'missing Stop timing step'
            assert timing['steps']['instance_stop']['calls'] == 1, 'sample must exercise one original Stop'
            request = value.get('request_id')
            assert request and request not in seen, 'every sampled choice needs a fresh request'
            seen.add(request)
        print('MODE_SAMPLE_ROUND ' + str(index+1), file=sys.stderr)


@contextmanager
def cpu_load():
    """Two local hash workers; no programs, files, accounts or scheduler effects."""
    stop = threading.Event()
    def work():
        body = b'x' * (64 << 10)
        while not stop.is_set():
            hashlib.sha256(body).digest()  # Releases the GIL for this input size.
    workers = [threading.Thread(target=work, name='zerus-native-sample-load') for _ in range(2)]
    try:
        for worker in workers: worker.start()
        yield
    finally:
        stop.set()
        for worker in workers: worker.join(timeout=2)
        assert not any(worker.is_alive() for worker in workers), 'sample load did not settle'


# These classes are selected explicitly by dispatch-only CI steps; they are not
# imported by, or added to, the ordinary native suites.


@unittest.skipUnless(os.name == 'nt', 'native Windows sampling')
class TransportSamples(unittest.TestCase):
    def test_loaded_sustained_streams(self):
        from concurrent.futures import ThreadPoolExecutor
        from test_remote_windows import WindowsNativeTransport
        rounds = round_count()
        self.assertGreater(rounds, 0, 'sampling requires an explicit positive count')
        with cpu_load(), ThreadPoolExecutor(max_workers=4) as pool:
            for index in range(rounds):
                cases = [WindowsNativeTransport('run_decoder') for _ in range(4)]
                futures = [pool.submit(case.run_decoder, 'powershell', sustained=True) for case in cases]
                errors = []
                for slot, future in enumerate(futures, 1):
                    try:
                        result = future.result()
                        self.assertTrue(result.get('marker_observed'), 'sample lacks its decoder input marker')
                        print('TRANSPORT_SAMPLE_CALL ' + json.dumps(dict(round=index+1, slot=slot, **result)),
                              file=sys.stderr)
                    except BaseException as error:
                        errors.append(error)
                # Join every in-flight fixture call. Only successful calls
                # confirm exit/capture; any error aborts further sampling.
                if errors:
                    raise errors[0]
                print('TRANSPORT_SAMPLE_ROUND ' + str(index+1), file=sys.stderr)


@unittest.skipUnless(os.name == 'nt', 'native Windows sampling')
class ModeSamples(unittest.TestCase):
    def test_serial_round_trips(self):
        from pathlib import Path
        import shutil
        from windows_frontend_auth import WindowsTaskMode
        rounds = round_count()
        self.assertGreater(rounds, 0, 'sampling requires an explicit positive count')

        class OriginalFixture(WindowsTaskMode):
            def sample(self):
                initial = self.cli('desktop', 'registration', 'status', '--json')
                self.assertFalse(initial['registered'], 'sampling requires an unused disposable runner')
                self.task = initial['task_name']
                self.sid = self.task.removeprefix('hgs-desktop-')
                clean = False
                try:
                    installed = self.cli('desktop', 'registration', 'install', '--json')
                    self.installed = True
                    self.cli_image = Path(installed['cli']['path']).resolve(strict=True)
                    shutil.copy2(self.frontend_source, self.frontend)
                    passive = self.mode('passive')
                    self.assertTrue(passive['ok'], passive)
                    self.latest = passive['instance']
                    def choose(desired):
                        value = self.mode(desired)
                        if value.get('ok') is True:
                            self.latest = value['instance']
                        return value
                    with cpu_load():
                        mode_rounds(rounds, self.grant, choose)
                    self.assertTrue(self.stop_exact_passive(self.latest)['stopped'])
                    self.assertFalse(self.cli('desktop', 'registration', 'remove', '--json')['registered'])
                    self.installed = False
                    clean = True
                finally:
                    if clean:
                        shutil.rmtree(self.root)
                    else:
                        print('MODE_SAMPLE_PRIVATE_ROOT_RETAINED ' + str(self.root), file=sys.stderr)

        result = unittest.TestResult()
        OriginalFixture('sample').run(result)
        self.assertFalse(result.skipped, result.skipped)
        self.assertFalse(result.errors + result.failures, result.errors + result.failures)
