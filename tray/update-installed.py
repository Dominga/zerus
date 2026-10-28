#!/usr/bin/env python3
"""Replace the installed desktop GUI with a new build, keeping drafts and native agents.

    update-installed.py <built hgs-tray | hgs-tray.app> <installed path>
    update-installed.py --rollback <installed path>
    update-installed.py --installed-gui <installed path>
    update-installed.py --check <built hgs-tray | hgs-tray.app> <installed path> [--live]

tray/install.sh runs this whenever its build differs from the installed GUI.

1. Copy the build into a unique folder beside the installed GUI.
2. If the installed GUI is running, quit it the way its Quit action does:
   SIGTERM when the process handles it (current builds flush drafts on that
   signal), otherwise the tray menu over D-Bus on Linux or the quit event on
   macOS. A GUI that does not quit keeps running and nothing is replaced.
3. Move the installed GUI aside into that folder and the new GUI into place.
   No GUI is deleted, and the one rollback copy
   (~/.local/share/hgs/desktop-previous) does not change, until the new GUI
   was verified running. Then the replaced GUI becomes the rollback copy, or,
   once the Tauri client replaced the Qt GUI, goes while Qt stays that copy.
   A failure or interrupt restores the replaced GUI or keeps it in a
   .hgs-tray-kept-* folder and prints where; so does an update of a GUI that
   is not running, which starts nothing. `--rollback` reinstalls the rollback
   copy through the same steps. Links at the installed path, the rollback
   folder or its copy are refused.
4. Start a GUI that was running through its service manager and wait for its
   single-instance socket; a new GUI that does not start is replaced by the
   previous one again. Then confirm the running image, the unchanged nonempty
   drafts and the native agent and tmux processes, and record
   ~/.local/share/hgs/build-provenance.json. Drafts of Qt and of the Tauri
   client (`<app data>/drafts`) are both snapshotted and verified. At a
   Qt-to-client switch on macOS, Qt's settings are exported beside the rollback
   copy and checked afterwards.
5. Remove hidden leftover folders of earlier hand-made updates beside the
   installed GUI (.zerus-*, stale .hgs-tray-update-*) when they are at least an
   hour old, hold only installer copies and no process uses them.

A numbered Dev installation is replaced only by a numbered build or a local
preview build (DEV_GUARD). tray/install.sh asks `--check` right after its
build, before it changes anything; without `--live` (HGS_TRAY_RESTART=0, which
replaces the GUI on disk only) it also refuses a switch between Qt and the
client.

Native agents, tmux servers and other services are never signalled. When a check
fails, the private draft snapshot is kept and its location printed.
HGS_TRAY_QUIT_TIMEOUT and HGS_TRAY_START_TIMEOUT (seconds, default 35) bound the
waits for the old GUI to quit and the new one to start.
"""
import argparse
import datetime
import errno
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time

MAC = sys.platform == 'darwin'
HOME = Path.home()
SHARE = HOME / '.local/share/hgs'
ROLLBACK = SHARE / 'desktop-previous'
PROVENANCE = SHARE / 'build-provenance.json'
SNAPSHOTS = HOME / '.local/state/hgs/desktop-update'
CLI = HOME / '.local/bin/hgs'
HELPER = HOME / '.local/bin/hgs_state.py'
LABEL = 'com.hgdev.hgs-tray'
PLIST = HOME / f'Library/LaunchAgents/{LABEL}.plist'
UNIT = 'hgs-tray.service'
NATIVE = {'codex', 'claude', 'kimi', 'dsh', 'node', 'codex-code-mode'}
# Hidden folders that earlier hand-made updates (and an interrupted run of this
# one) left beside the installed GUI, and the installer copies they may hold.
UPDATE_PREFIX = '.hgs-tray-update-'
LEGACY = ('.zerus-', UPDATE_PREFIX)
# An update folder that keeps a replaced GUI: never removed automatically.
KEPT_PREFIX = '.hgs-tray-kept-'
LEGACY_FILES = {'hgs-tray', 'hgs', 'hgs_state.py'}
LEGACY_AGE = 3600
# Slow machines may allow longer; the defaults match the verified manual updates.
QUIT_SECONDS = float(os.environ.get('HGS_TRAY_QUIT_TIMEOUT') or 35)
START_SECONDS = float(os.environ.get('HGS_TRAY_START_TIMEOUT') or 35)
LSREGISTER = Path('/System/Library/Frameworks/CoreServices.framework/Frameworks/'
                  'LaunchServices.framework/Support/lsregister')
DEFAULTS = 'defaults'
# Qt's QSettings domain, which is also the NSUserDefaults domain of any bundle
# with this identifier (the Tauri switch bundle included).
PREFERENCES = 'com.hgdev.hgs-tray'
# Kept beside the Qt rollback copy at the switch (docs/macos-distribution.md).
SETTINGS_SNAPSHOT = ROLLBACK / f'{PREFERENCES}.plist'
# Keys AppKit and WebKit write into a bundle's defaults domain; every other key is Qt's.
COCOA_PREFIXES = ('NS', 'Apple', 'Web', 'com.apple.', '_')


class UpdateError(Exception):
    pass


def say(text):
    print(f'hgs-tray update: {text}', flush=True)


def run(args, **kwargs):
    return subprocess.check_output([str(a) for a in args], text=True, stderr=subprocess.PIPE, **kwargs).strip()


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def digest(path):
    hasher = hashlib.sha256()
    with Path(path).open('rb') as f:
        for block in iter(lambda: f.read(1 << 20), b''):
            hasher.update(block)
    return hasher.hexdigest()


def executable(gui):
    return gui / 'Contents/MacOS/hgs-tray' if MAC else gui


def drafts_dir():
    if MAC:
        return HOME / 'Library/Preferences/com.hgdev.hgs-tray.plist.drafts'
    config = os.environ.get('XDG_CONFIG_HOME') or str(HOME / '.config')
    return Path(config) / 'hgdev/hgs-tray.conf.drafts'


def remove(path):
    if path.is_dir() and not path.is_symlink():
        shutil.rmtree(path)
    elif path.exists() or path.is_symlink():
        path.unlink()


def write_json(path, data):
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    fd, temporary = tempfile.mkstemp(dir=path.parent, prefix=f'.{path.name}.')
    with os.fdopen(fd, 'w') as f:
        json.dump(data, f, indent=2)
        f.write('\n')
    os.replace(temporary, path)


# --- processes -----------------------------------------------------------------

def gui_pids(installed):
    """This user's processes running the installed GUI (never one from another path)."""
    target = str(executable(installed))
    pids = []
    if MAC:
        for line in run(['ps', '-x', '-o', 'pid=,comm=']).splitlines():
            pid, _, command = line.strip().partition(' ')
            if command.strip() == target:
                pids.append(int(pid))
        return sorted(pids)
    for process in Path('/proc').iterdir():
        if not process.name.isdigit():
            continue
        try:
            if process.stat().st_uid != os.getuid() or (process / 'comm').read_text().strip() != 'hgs-tray':
                continue
            exe = os.readlink(process / 'exe').removesuffix(' (deleted)')
            argv = [part.decode(errors='replace') for part in (process / 'cmdline').read_bytes().split(b'\0')[:3]]
        except OSError:
            continue
        # A script started through its #! line has its path among the first arguments.
        if exe == target or target in argv:
            pids.append(int(process.name))
    return sorted(pids)


def alive(pid):
    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    if not MAC:
        # An exited process its parent has not reaped yet is gone.
        try:
            return Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[0] != 'Z'
        except (OSError, IndexError):
            return False
    return True


def native():
    """Native agents and tmux servers of this user with their start times, keyed by PID."""
    records = {}
    if MAC:
        for line in run(['ps', '-x', '-o', 'pid=,lstart=,comm=']).splitlines():
            parts = line.split(None, 6)
            if len(parts) == 7:
                name = Path(parts[6]).name
                if name in NATIVE or name.startswith('tmux'):
                    records[parts[0]] = {'name': name, 'start': ' '.join(parts[1:6])}
        return records
    for process in Path('/proc').iterdir():
        if not process.name.isdigit():
            continue
        try:
            if process.stat().st_uid != os.getuid():
                continue
            name = (process / 'comm').read_text().strip()
            if name in NATIVE or name.startswith('tmux'):
                fields = (process / 'stat').read_text().rsplit(')', 1)[1].split()
                records[process.name] = {'name': name, 'start': fields[19]}
        except OSError:
            pass
    return records


def socket_path(pid):
    """The GUI's single-instance socket, read from the process itself."""
    if MAC:
        try:
            names = run(['lsof', '-n', '-a', '-p', str(pid), '-U', '-F', 'n'])
        except (OSError, subprocess.CalledProcessError):
            return None
        found = [line[1:] for line in names.splitlines() if line.startswith('n') and line.endswith('/hgs-tray')]
        return found[0] if found else None
    inodes = set()
    for fd in Path(f'/proc/{pid}/fd').iterdir():
        try:
            match = re.fullmatch(r'socket:\[(\d+)\]', os.readlink(fd))
        except OSError:
            continue
        if match:
            inodes.add(match.group(1))
    for line in Path('/proc/net/unix').read_text().splitlines()[1:]:
        fields = line.split()
        if len(fields) >= 8 and fields[6] in inodes and fields[7].endswith('/hgs-tray'):
            return fields[7]
    return None


def handles_sigterm(pid):
    if MAC:
        return False  # not observable here; the quit event works for every build
    for line in Path(f'/proc/{pid}/status').read_text().splitlines():
        if line.startswith('SigCgt:'):
            return bool(int(line.split()[1], 16) & (1 << (signal.SIGTERM - 1)))
    return False


# --- quitting and starting ------------------------------------------------------

def menu_quit(pid):
    """Click Quit in the tray menu that this process owns (StatusNotifierItem)."""
    try:
        items = run(['qdbus6', 'org.kde.StatusNotifierWatcher', '/StatusNotifierWatcher',
                     'org.kde.StatusNotifierWatcher.RegisteredStatusNotifierItems']).splitlines()
    except (OSError, subprocess.CalledProcessError):
        items = []
    services = []
    for item in items:
        service = item.split('/')[0]
        try:
            owner = int(run(['qdbus6', 'org.freedesktop.DBus', '/org/freedesktop/DBus',
                             'org.freedesktop.DBus.GetConnectionUnixProcessID', service]))
        except (OSError, ValueError, subprocess.CalledProcessError):
            continue
        if owner == pid:
            services.append(service)
    if len(services) != 1:
        raise UpdateError('the running Zerus predates graceful service stops and its tray menu is not reachable; '
                          'quit Zerus from its tray menu, then run the installer again')
    layout = json.loads(run(['busctl', '--user', '--json=short', '--', 'call', services[0], '/MenuBar',
                             'com.canonical.dbusmenu', 'GetLayout', 'iias', '0', '-1', '0']))
    actions = []

    def walk(node):
        if isinstance(node, dict):
            for child in node.values():
                walk(child)
        elif isinstance(node, list):
            if len(node) == 3 and isinstance(node[0], int) and isinstance(node[1], dict):
                if node[1].get('label', {}).get('data', '').replace('&', '') == 'Quit':
                    actions.append(node[0])
            for child in node:
                walk(child)

    walk(layout)
    if len(actions) != 1:
        raise UpdateError('cannot find the Quit action in the running Zerus tray menu; quit it, then run the installer again')
    run(['busctl', '--user', '--', 'call', services[0], '/MenuBar', 'com.canonical.dbusmenu',
         'Event', 'isvu', str(actions[0]), 'clicked', 'i', '0', '0'])


def quit_gui(pid, installed):
    if handles_sigterm(pid):
        os.kill(pid, signal.SIGTERM)
    elif MAC:
        run(['osascript', '-e', f'ignoring application responses\ntell application id "{LABEL}" to quit\nend ignoring'])
    else:
        menu_quit(pid)
    deadline = time.monotonic() + QUIT_SECONDS
    while alive(pid) and time.monotonic() < deadline:
        time.sleep(0.25)
    if alive(pid):
        raise UpdateError(f'Zerus (pid {pid}) did not quit within {QUIT_SECONDS:g} s, for example while a swarm change '
                          'is in progress; it keeps running and nothing was replaced')
    time.sleep(1)
    if gui_pids(installed):
        raise UpdateError('Zerus started again during the update; nothing was replaced')


def start(installed):
    if MAC:
        domain = f'gui/{os.getuid()}'
        subprocess.run(['launchctl', 'bootout', f'{domain}/{LABEL}'], capture_output=True)
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline and subprocess.run(
                ['launchctl', 'print', f'{domain}/{LABEL}'], capture_output=True).returncode == 0:
            time.sleep(0.25)
        run(['launchctl', 'bootstrap', domain, PLIST])
    else:
        run(['systemctl', '--user', 'start', UNIT])


def wait_started(installed, path, old_pid):
    deadline = time.monotonic() + START_SECONDS
    while time.monotonic() < deadline:
        pids = [pid for pid in gui_pids(installed) if pid != old_pid]
        if len(pids) == 1:
            if path is None:
                time.sleep(2)
                if alive(pids[0]):
                    return pids[0]
            else:
                try:
                    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as client:
                        client.settimeout(1)
                        client.connect(path)
                        # The window that was open before the update comes back.
                        client.sendall(b'sessions\n')
                    return pids[0]
                except OSError:
                    pass
        time.sleep(0.5)
    return None


def stop_started(installed):
    for pid in gui_pids(installed):
        os.kill(pid, signal.SIGTERM)
        deadline = time.monotonic() + QUIT_SECONDS
        while alive(pid) and time.monotonic() < deadline:
            time.sleep(0.25)


# --- files and checks -----------------------------------------------------------

def bundle_info(bundle):
    """A bundle's Info.plist; {} without one (a Linux binary, a missing bundle)."""
    try:
        return plistlib.loads((Path(bundle) / 'Contents/Info.plist').read_bytes())
    except (OSError, ValueError, plistlib.InvalidFileException):
        return {}


def gui_kind(info):
    """'tauri' for the Tauri client's switch bundle (Info.plist ZerusGUI), else 'qt'."""
    return 'tauri' if info.get('ZerusGUI') == 'tauri' else 'qt'


def bundle_kind(bundle):
    """The GUI kind of a bundle whose Info.plist names it; None for a missing
    or half-deleted one, which must not count as Qt."""
    info = bundle_info(bundle)
    return gui_kind(info) if info.get('CFBundleIdentifier') else None


def keeps_slot(installed_kind, slot_kind, new_kind):
    """Whether the one rollback copy stays. Once it holds Qt, it stays the
    switch's rollback target while the client replaces anything but Qt
    itself: a client, an incomplete bundle or nothing. The replaced client
    waits only for this update's own restore."""
    return slot_kind == 'qt' and new_kind == 'tauri' and installed_kind != 'qt'


def client_drafts_root(identifier):
    """The Tauri client's drafts root, `<app data>/drafts`: Tauri's
    app_data_dir formula with the running bundle's identifier."""
    if MAC:
        return HOME / 'Library/Application Support' / identifier / 'drafts'
    return Path(os.environ.get('XDG_DATA_HOME') or str(HOME / '.local/share')) / identifier / 'drafts'


def draft_roots(*infos):
    """Qt's drafts root, then the drafts root of each Tauri client among the bundles."""
    roots = [('qt', drafts_dir())]
    for info in infos:
        identifier = info.get('CFBundleIdentifier')
        if gui_kind(info) == 'tauri' and identifier:
            root = (f'client-{identifier}', client_drafts_root(identifier))
            if root not in roots:
                roots.append(root)
    return roots


def register(bundle):
    """Refresh LaunchServices' record of the installed bundle; never a global reset."""
    if MAC and LSREGISTER.exists():
        subprocess.run([str(LSREGISTER), '-f', str(bundle)], capture_output=True)


def unregister(bundle):
    """Forget a bundle copy that left the installed path, so a notification
    click or `open -b com.hgdev.hgs-tray` cannot launch it."""
    if MAC and LSREGISTER.exists() and Path(bundle).suffix == '.app':
        subprocess.run([str(LSREGISTER), '-u', str(bundle)], capture_output=True)


def snapshot(destination, roots):
    """Private copies of every drafts root, one folder per root."""
    destination.mkdir(mode=0o700)
    count = 0
    for label, source in roots:
        target = destination / label
        if not source.exists():
            target.mkdir(mode=0o700)
            continue
        if source.is_symlink() or any(p.is_symlink() for p in source.rglob('*')):
            raise UpdateError(f'draft storage {source} contains symbolic links; nothing was replaced')
        shutil.copytree(source, target)
        for p in target.rglob('*'):
            os.chmod(p, 0o700 if p.is_dir() else 0o600)
        os.chmod(target, 0o700)
        count += sum(1 for _ in target.rglob('draft.json'))
    return count


def changed_drafts(saved, roots):
    """Nonempty drafts saved when the old GUI quit that are missing or different now."""
    problems, preserved = [], 0
    for label, current in roots:
        prefix = '' if label == 'qt' else f'{label} '
        kept = saved / label
        for before in sorted(kept.rglob('draft.json')):
            relative = before.relative_to(kept).parent
            original = json.loads(before.read_text())
            if not (original.get('text') or original.get('attachments') or original.get('form_state')):
                continue
            after = current / relative / 'draft.json'
            if not after.exists():
                problems.append(f'{prefix}{relative}: missing')
                continue
            restored = json.loads(after.read_text())
            if any(original.get(key) != restored.get(key) for key in ('key', 'text', 'attachments', 'form_state')):
                problems.append(f'{prefix}{relative}: changed')
                continue
            for attachment in original.get('attachments') or []:
                blob = after.parent / (attachment['sha256'] + '.bin')
                if not blob.exists() or digest(blob) != attachment['sha256']:
                    problems.append(f'{prefix}{relative}: attachment changed')
            preserved += 1
    return preserved, problems


def running_image_matches(pid, installed):
    exe = executable(installed)
    if MAC:
        text = run(['lsof', '-n', '-a', '-p', str(pid), '-d', 'txt', '-F', 'ni'])
        entry = []
        for line in text.splitlines():
            if line.startswith('f'):
                entry = []
            entry.append(line)
            if line == 'n' + str(exe):
                return 'i' + str(exe.stat().st_ino) in entry
        return False
    running = Path(f'/proc/{pid}/exe')
    if os.readlink(running) == str(exe):
        return running.stat().st_ino == exe.stat().st_ino
    # A script GUI: its interpreter runs the installed file.
    return str(exe) in (Path(f'/proc/{pid}/cmdline').read_bytes().decode(errors='replace').split('\0')[:3])


def build_info(path):
    try:
        info = json.loads(run([executable(path), '--build-info'], timeout=30))
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        raise UpdateError(f'{path} does not report its build identity: {error}') from None
    if not isinstance(info, dict) or not info.get('version'):
        raise UpdateError(f'{path} reports an invalid build identity')
    return info


def provenance(installed, info, installed_at, preserved, retained):
    record = {'schema': 1, 'sourceCommit': info.get('sourceCommit') or None, 'sourceDirty': info.get('sourceDirty'),
              'guiVersion': info.get('version'), 'guiVersionName': info.get('versionName'),
              'guiSha256': digest(executable(installed)), 'installedAt': installed_at, 'verifiedAt': now(),
              'preservedDrafts': preserved, 'nativeRetained': retained}
    if CLI.exists():
        record['cliSha256'] = digest(CLI)
        try:
            record['cliVersion'] = run([CLI, '--version'], timeout=30)
        except (OSError, subprocess.SubprocessError):
            record['cliVersion'] = None
    if HELPER.exists():
        record['helperSha256'] = digest(HELPER)
    write_json(PROVENANCE, record)


# --- leftovers of earlier updates -------------------------------------------------

def legacy_copies(folder):
    """Installer copies inside a leftover folder, or None when it holds anything else."""
    copies = []
    for root, dirs, files in os.walk(folder):
        base = Path(root)
        depth = len(base.relative_to(folder).parts)
        for name in list(dirs):
            path = base / name
            if path.is_symlink():
                return None
            if MAC and name == 'hgs-tray.app':
                copies.append(path)
                dirs.remove(name)
            elif depth >= 1 or (name == 'previous' and folder.name.startswith(UPDATE_PREFIX)):
                # This updater's set-aside GUI may be the only copy of it.
                return None
        for name in files:
            path = base / name
            if MAC and name == '.DS_Store':
                continue
            if path.is_symlink() or name not in LEGACY_FILES:
                return None
            copies.append(path)
    return copies


def in_use(folder):
    """Whether a process of this user runs from, or sits in, the folder."""
    prefix = str(folder) + os.sep
    if MAC:
        return any(line.strip().partition(' ')[2].strip().startswith(prefix)
                   for line in run(['ps', '-x', '-o', 'pid=,comm=']).splitlines())
    for process in Path('/proc').iterdir():
        if not process.name.isdigit():
            continue
        try:
            if process.stat().st_uid != os.getuid():
                continue
            paths = [os.readlink(process / 'exe'), os.readlink(process / 'cwd')]
            paths += [part.decode(errors='replace') for part in (process / 'cmdline').read_bytes().split(b'\0')[:3]]
        except OSError:
            continue
        if any(path.startswith(prefix) for path in paths):
            return True
    return False


def sweep(parent, current):
    """Remove leftover staging folders that are empty or hold only unused installer copies."""
    removed = []
    for folder in sorted(parent.iterdir()):
        if (not folder.name.startswith(LEGACY) or folder == current or folder.is_symlink()
                or not folder.is_dir()):
            continue
        # A concurrent update may still be using a recent folder.
        if time.time() - folder.stat().st_mtime < LEGACY_AGE:
            continue
        copies = legacy_copies(folder)
        if copies is None:
            say(f'kept {folder}: it holds files other than installer copies')
            continue
        if in_use(folder):
            say(f'kept {folder}: a process still uses it')
            continue
        for bundle in copies:
            unregister(bundle)
        shutil.rmtree(folder)
        removed.append(folder.name)
    if removed:
        say(f'removed {len(removed)} leftover folders of earlier updates: {", ".join(removed)}')


# --- the update ---------------------------------------------------------------

def refuse_links(installed):
    """Never follow a link at the installed path, the rollback folder or its slot."""
    for path in (installed, ROLLBACK, ROLLBACK / installed.name):
        if path.is_symlink():
            raise UpdateError(f'{path} is a symbolic link; nothing was replaced')


def rollback_dir():
    """The private folder of the one rollback copy and the Qt settings copy."""
    if ROLLBACK.is_symlink():
        raise UpdateError(f'{ROLLBACK} is a symbolic link; nothing was replaced')
    ROLLBACK.mkdir(mode=0o700, parents=True, exist_ok=True)
    os.chmod(ROLLBACK, 0o700)
    return ROLLBACK


def put_back(installed, previous, new):
    """Move the GUI at the installed path to `new` and `previous` back into place."""
    if installed.exists() or installed.is_symlink():
        new.parent.mkdir(parents=True, exist_ok=True)
        os.rename(installed, new)
    os.rename(previous, installed)
    register(installed)


def swap_in(stage, installed, aside):
    """Move the installed GUI to `aside` and the stage into place, registered.
    Nothing is deleted and the rollback copy does not change; on any failure
    or interrupt the replaced GUI moves back."""
    present = installed.exists()
    if present:
        aside.parent.mkdir(parents=True, exist_ok=True)
        os.rename(installed, aside)
    try:
        os.rename(stage, installed)
        if MAC:
            run(['codesign', '--verify', '--deep', '--strict', installed])
            register(installed)
        if present:
            unregister(aside)
    except BaseException:
        if present and aside.exists():
            put_back(installed, aside, stage)
        raise


def take_slot(previous):
    """After a verified start, the replaced GUI becomes the one rollback copy,
    unregistered. The old copy leaves the slot only once the new one is in it;
    an interrupted step puts the old copy back and says where the new one is."""
    slot = rollback_dir() / previous.name
    incoming = Path(tempfile.mkdtemp(prefix='.incoming-', dir=ROLLBACK)) / previous.name
    replaced = None
    try:
        try:
            os.rename(previous, incoming)
        except OSError as error:
            if error.errno != errno.EXDEV:
                raise
            # Another file system: the original goes once the copy is in the slot.
            if previous.is_dir():
                shutil.copytree(previous, incoming, symlinks=True)
            else:
                shutil.copy2(previous, incoming)
        if slot.exists():
            replaced = Path(tempfile.mkdtemp(prefix='.replaced-', dir=ROLLBACK)) / slot.name
            os.rename(slot, replaced)
        os.rename(incoming, slot)
    except BaseException:
        if replaced is not None and replaced.exists() and not slot.exists():
            os.rename(replaced, slot)
        where = [str(path) for path in (incoming, previous) if path.exists()]
        say(f'the rollback copy {slot} did not change; the replaced GUI is kept in {" and ".join(where)}')
        raise
    incoming.parent.rmdir()
    unregister(slot)
    if previous.exists():
        remove(previous)
    if replaced is not None:
        shutil.rmtree(replaced.parent)


def settle(previous, keep):
    """Only after the new GUI was verified running: the GUI it replaced becomes
    the rollback copy, or goes when the slot keeps Qt (a superseded client) or
    when it was no complete bundle. Says which."""
    if previous is None:
        return 'nothing was installed before'
    if MAC and bundle_kind(previous) is None:
        remove(previous)
        return 'the incomplete bundle that was installed (no Info.plist identity) was removed'
    if keep:
        remove(previous)
        return f'the Qt GUI stays the rollback copy in {ROLLBACK}'
    take_slot(previous)
    return f'the replaced GUI is the rollback copy in {ROLLBACK}'


def keep_aside(folder, aside):
    """An update folder that still holds the replaced GUI is kept, renamed so
    that sweep() never removes it; any other is removed, since it holds only
    copies of the build. Returns the kept GUI's path, or None."""
    if not (aside.exists() or aside.is_symlink()):
        shutil.rmtree(folder)
        return None
    kept = folder.with_name(KEPT_PREFIX + folder.name[len(UPDATE_PREFIX):])
    try:
        os.rename(folder, kept)
    except BaseException:
        # sweep() keeps a folder with a set-aside GUI under its own name too.
        return aside
    return kept / aside.relative_to(folder)


def report_kept(installed):
    """Copies that earlier updates kept for recovery: listed, never removed."""
    kept = sorted(installed.parent.glob(KEPT_PREFIX + '*')) if installed.parent.is_dir() else []
    if ROLLBACK.is_dir() and not ROLLBACK.is_symlink():
        kept += sorted(path for path in ROLLBACK.iterdir()
                       if path.name.startswith(('.incoming-', '.replaced-')) and path.is_dir() and any(path.iterdir()))
    if kept:
        say(f'earlier updates kept these copies; remove them once Zerus runs as it should: {", ".join(map(str, kept))}')


def recovery(installed, expected, verified, kept):
    """Where things are after an update that did not finish."""
    try:
        holds = 'new' if digest(executable(installed)) == expected else 'previous'
    except OSError:
        holds = None
    parts = [{None: f'nothing complete is installed at {installed}',
              'previous': f'{installed} keeps the previous GUI',
              'new': f'{installed} holds the new GUI' + ('' if verified else ', not verified running')}[holds]]
    if kept:
        parts.append(f'the replaced GUI is kept in {kept}; to put it back, quit Zerus and move it to {installed}')
    return 'not finished: ' + '; '.join(parts)


def installed_build_info(installed):
    """The installed GUI's --build-info, None when it cannot tell (an old build)."""
    try:
        return build_info(installed)
    except UpdateError:
        return None


# The rule as docs/reference.md states it ("Installation, configuration and
# diagnostics"); a test keeps the two equal.
DEV_GUARD = ('A numbered Dev installation is replaced only by a numbered Dev build or by a local preview build: '
             'configure tray/build with -DZERUS_DEV_BUILD_NUMBER=N, or build the Tauri client with '
             'ZERUS_DEV_BUILD_NUMBER=N in its environment, N being the CI sequence of the main push the build comes from, '
             'or build an owner-requested branch preview with ZERUS_LOCAL_PREVIEW_LABEL.')


def numbered_refusal(installed_info, new_info):
    """Why a build may not replace the installed GUI: a numbered Dev install
    is replaced only by a numbered build or an explicit local preview (AGENTS.md)."""
    installed = int((installed_info or {}).get('devBuildNumber') or 0)
    if installed > 0 and not int(new_info.get('devBuildNumber') or 0) and not new_info.get('previewLabel'):
        return (f'the installed GUI is Dev build {installed} and this build has neither a Dev number '
                f'nor a local preview label. {DEV_GUARD}')
    return None


def check_replace(built, installed, live=False):
    """Before tray/install.sh changes anything: the numbered Dev guard, and,
    for an install that replaces the GUI on disk without the live update
    (HGS_TRAY_RESTART=0, not `live`), no change between Qt and the client on
    macOS, which only the live update may make."""
    built, installed = Path(built).resolve(), Path(installed).absolute()
    if not installed.exists():
        return 0
    refusal = numbered_refusal(installed_build_info(installed), build_info(built))
    if refusal:
        raise UpdateError(refusal + ' Nothing was replaced.')
    if not live and MAC and bundle_kind(built) != bundle_kind(installed):
        raise UpdateError('switching between the Qt GUI and the Tauri client needs the live update; '
                          'run tray/install.sh without HGS_TRAY_RESTART=0. Nothing was replaced.')
    return 0


def export_settings():
    """Qt's settings domain as cfprefsd serves it (never the plist file), or None."""
    try:
        result = subprocess.run([DEFAULTS, 'export', PREFERENCES, '-'], capture_output=True, timeout=30)
    except (OSError, subprocess.TimeoutExpired):
        return None
    if result.returncode:
        return None
    try:
        return result.stdout, plistlib.loads(result.stdout)
    except (ValueError, plistlib.InvalidFileException):
        return None


def cocoa_key(key):
    return key.startswith(COCOA_PREFIXES)


def value_bytes(value):
    return plistlib.dumps({'value': value}, fmt=plistlib.FMT_XML, sort_keys=True)


def settings_changes(before, after):
    """Qt keys that changed or vanished, other non-Cocoa keys that appeared,
    and the Cocoa keys written, each sorted."""
    changed = sorted(k for k, v in before.items()
                     if not cocoa_key(k) and (k not in after or value_bytes(after[k]) != value_bytes(v)))
    added = sorted(k for k in after if k not in before and not cocoa_key(k))
    cocoa = sorted(k for k, v in after.items()
                   if cocoa_key(k) and (k not in before or value_bytes(before[k]) != value_bytes(v)))
    return changed, added, cocoa


def save_settings_snapshot():
    """At the switch, keep Qt's settings beside the Qt rollback copy (0600)."""
    exported = export_settings()
    if exported is None:
        raise UpdateError(f'cannot read the {PREFERENCES} settings to keep a copy; nothing was replaced')
    rollback_dir()
    fd, temporary = tempfile.mkstemp(dir=SETTINGS_SNAPSHOT.parent, prefix=f'.{SETTINGS_SNAPSHOT.name}.')
    with os.fdopen(fd, 'wb') as f:
        f.write(exported[0])
    os.replace(temporary, SETTINGS_SNAPSHOT)
    say(f'kept a copy of the Qt settings in {SETTINGS_SNAPSHOT}')


def settings_report(before_qt=False):
    """Qt's keys against the copy kept at the switch: problems and notes.
    `before_qt`: the check before Qt runs again, which can still restore them."""
    if not SETTINGS_SNAPSHOT.exists():
        return [], []
    exported = export_settings()
    if exported is None:
        return [f'cannot read the {PREFERENCES} settings to compare them with {SETTINGS_SNAPSHOT}'], []
    try:
        kept = plistlib.loads(SETTINGS_SNAPSHOT.read_bytes())
    except (OSError, ValueError) as error:
        kept = error
    if not isinstance(kept, dict):
        return [f'cannot read the copy of the Qt settings {SETTINGS_SNAPSHOT} ({kept})'], []
    changed, added, cocoa = settings_changes(kept, exported[1])
    problems = []
    if changed:
        problems.append(f'Qt settings changed or missing: {", ".join(changed)} (the copy is {SETTINGS_SNAPSHOT})')
        if before_qt:
            problems.append(f'with Zerus quit, restore them with: defaults import {PREFERENCES} {SETTINGS_SNAPSHOT}')
    if added:
        problems.append(f"settings written that are neither Qt's nor Cocoa's: {', '.join(added)}")
    notes = [f'Cocoa settings written since the switch: {", ".join(cocoa)}'] if cocoa else []
    return problems, notes


def update(built, installed, rollback=False):
    built, installed = built.resolve(), installed.absolute()
    refuse_links(installed)
    info = build_info(built)
    slot = ROLLBACK / installed.name
    new_info = bundle_info(built) if MAC else {}
    old_info = bundle_info(installed) if MAC else {}
    old_kind = bundle_kind(installed) if MAC else None
    new_kind = bundle_kind(built) if MAC else None
    if not rollback and installed.exists():
        refusal = numbered_refusal(installed_build_info(installed), info)
        if refusal:
            raise UpdateError(refusal + ' Nothing was replaced.')
    switching = MAC and old_kind == 'qt' and new_kind == 'tauri'
    # Qt's settings follow the direction of the change: leaving the client for
    # Qt checks them before Qt runs again and writes its own (and can still
    # restore them); a client checks them once it runs.
    to_qt = MAC and old_kind == 'tauri' and new_kind == 'qt'
    to_client = MAC and new_kind == 'tauri'
    # A rollback installs from the slot, whose copy stays until the new GUI runs.
    keep = not rollback and keeps_slot(old_kind, bundle_kind(slot) if MAC else None, new_kind)
    roots = draft_roots(old_info, new_info)
    name = info.get('versionName', info['version'])
    report_kept(installed)
    installed.parent.mkdir(parents=True, exist_ok=True)
    folder = Path(tempfile.mkdtemp(prefix=UPDATE_PREFIX, dir=installed.parent))
    stage = folder / installed.name
    # The replaced GUI waits here until the new one was verified running; only
    # then may it leave (settle()). A failure or interrupt keeps or restores it.
    aside = folder / 'previous' / installed.name
    saved = expected = None
    verified = done = False
    try:
        if MAC:
            run(['codesign', '--verify', '--deep', '--strict', built])
            shutil.copytree(built, stage, symlinks=True)
            run(['codesign', '--verify', '--deep', '--strict', stage])
        else:
            shutil.copy2(built, stage)
            os.chmod(stage, 0o755)
        expected = digest(executable(stage))
        pids = gui_pids(installed)
        if len(pids) > 1:
            raise UpdateError(f'several copies of Zerus are running ({pids}); quit them, then run the installer again')
        if not pids:
            early = settings_report(before_qt=True)[0] if to_qt else []
            if switching:
                save_settings_snapshot()
            swap_in(stage, installed, aside)
            # Nothing starts the new GUI, so nothing verifies it: the rollback
            # copy stays and the replaced GUI is kept.
            kept = keep_aside(folder, aside)
            done = True
            say(f'installed {name} without starting it; ' + (
                f'the rollback copy did not change and the replaced GUI is kept in {kept}' if kept
                else 'nothing was installed before'))
            sweep(installed.parent, folder)
            if early:
                raise UpdateError('the new GUI is installed, but ' + '; '.join(early))
            return 0
        pid = pids[0]
        path, before = socket_path(pid), native()
        say(f'quitting Zerus (pid {pid}) so it saves its drafts')
        quit_gui(pid, installed)
        try:
            early = settings_report(before_qt=True)[0] if to_qt else []
            SNAPSHOTS.mkdir(mode=0o700, parents=True, exist_ok=True)
            saved = Path(tempfile.mkdtemp(prefix='drafts-', dir=SNAPSHOTS))
            os.rmdir(saved)
            snapshot(saved, roots)
            if switching:
                save_settings_snapshot()
            swap_in(stage, installed, aside)
        except Exception as error:
            # swap_in() moved the previous GUI back if it had moved it.
            try:
                start(installed)
            except Exception as again:
                raise UpdateError(f'{error}; the previous GUI did not start again: {again}') from None
            raise UpdateError(f'{error}; the previous GUI was started again') from None
        installed_at = now()
        try:
            start(installed)
            new = wait_started(installed, path, pid)
            failure = ''
        except Exception as error:
            new, failure = None, f' ({error})'
        if new is None:
            stop_started(installed)
            if not aside.exists():
                raise UpdateError('the new GUI did not start' + failure)
            put_back(installed, aside, folder / 'new' / installed.name)
            try:
                start(installed)
                wait_started(installed, path, pid)
            except Exception as error:
                raise UpdateError(f'the new GUI did not start{failure}; the previous GUI was restored '
                                  f'but did not start: {error}') from None
            raise UpdateError(f'the new GUI did not start{failure}; the previous GUI was restored and started')
        verified = True
        settled = settle(aside if aside.exists() else None, keep)
        done = True
        problems = list(early)
        if digest(executable(installed)) != expected or not running_image_matches(new, installed):
            problems.append('the running image is not the new build')
        preserved, draft_problems = changed_drafts(saved, roots)
        problems += draft_problems
        if to_client:
            late, notes = settings_report()
            problems += late
            for note in notes:
                say(note)
        after = native()
        replaced = [p for p, value in before.items() if p in after and after[p] != value]
        if replaced:
            problems.append(f'native process identities changed: {replaced}')
        if problems:
            raise UpdateError('the new GUI runs, but ' + '; '.join(problems))
        retained = sum(after.get(p) == value for p, value in before.items())
        provenance(installed, info, installed_at, preserved, retained)
        shutil.rmtree(saved)
        saved = None
        sweep(installed.parent, folder)
        say(f'Zerus {name} runs as pid {new}; {preserved} nonempty drafts kept, '
            f'{retained} native processes untouched; {settled}')
        return 0
    except UpdateError:
        if saved is not None and saved.exists():
            say(f'drafts saved when Zerus quit remain in {saved}')
        raise
    finally:
        kept = None
        try:
            if folder.exists():
                kept = keep_aside(folder, aside)
        finally:
            if not done:
                say(recovery(installed, expected, verified, kept))
            elif kept:
                say(f'the replaced GUI is kept in {kept}')


def roll_back_installed(installed):
    """Reinstall the one rollback copy (after the switch: the last Qt GUI)
    through the same quit, snapshot, start and verification as an update."""
    installed = installed.absolute()
    refuse_links(installed)
    previous = ROLLBACK / installed.name
    if not previous.exists():
        raise UpdateError(f'there is no rollback copy in {ROLLBACK}; nothing was replaced')
    say(f'reinstalling the rollback copy {previous}')
    return update(previous, installed, rollback=True)


def main():
    usage = __doc__.split('\n\n')[1]
    parser = argparse.ArgumentParser(usage=usage, add_help=False)
    parser.add_argument('--rollback', metavar='INSTALLED', type=Path)
    parser.add_argument('--installed-gui', metavar='INSTALLED', type=Path)
    parser.add_argument('--check', nargs=2, metavar=('BUILT', 'INSTALLED'), type=Path)
    parser.add_argument('--live', action='store_true')
    parser.add_argument('paths', nargs='*', type=Path)
    args = parser.parse_args()
    chosen = [option for option in (args.rollback, args.installed_gui, args.check) if option]
    try:
        if args.live and not args.check:
            chosen = []
        elif args.installed_gui and len(chosen) == 1 and not args.paths:
            print(bundle_kind(args.installed_gui) or 'none')
            return 0
        elif args.rollback and len(chosen) == 1 and not args.paths:
            return roll_back_installed(args.rollback)
        elif args.check and len(chosen) == 1 and not args.paths:
            return check_replace(*args.check, live=args.live)
        elif len(args.paths) == 2 and not chosen:
            return update(*args.paths)
    except UpdateError as error:
        print(f'hgs-tray update failed: {error}', file=sys.stderr)
        return 1
    print(usage, file=sys.stderr)
    return 2

if __name__ == '__main__':
    sys.exit(main())
