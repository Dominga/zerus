"""Bounded AT-SPI reads of the component fixture on its own two private buses."""
import ast
from contextlib import contextmanager
import os
from pathlib import Path
import subprocess
import time


def _owns(address, name):
    reply = subprocess.run(["gdbus", "call", "--address", address, "--dest", "org.freedesktop.DBus",
                            "--object-path", "/org/freedesktop/DBus", "--method",
                            "org.freedesktop.DBus.NameHasOwner", name],
                           text=True, capture_output=True, timeout=1)
    return reply.returncode == 0 and reply.stdout.strip() == "(true,)"


def _executable(name):
    for directory in ("/usr/libexec", "/usr/lib", "/usr/lib/at-spi2-core"):
        path = Path(directory, name)
        if path.is_file() and os.access(path, os.X_OK):
            return str(path)
    raise RuntimeError(f"Install at-spi2-core for the accessibility smoke: {name}")


@contextmanager
def private_bus(scratch):
    session = os.environ.get("DBUS_SESSION_BUS_ADDRESS", "")
    parent = os.environ.get("ZERUS_MENU_A11Y_PARENT_BUS")
    if not session or parent is None or session == parent:
        raise RuntimeError("Refusing an accessibility smoke without its own session bus")
    runtime = Path(scratch, "runtime")
    runtime.mkdir(mode=0o700)
    os.environ["XDG_RUNTIME_DIR"] = str(runtime)
    os.environ.pop("AT_SPI_BUS_ADDRESS", None)
    children = []
    try:
        children.append(subprocess.Popen([_executable("at-spi-bus-launcher"), "--launch-immediately"],
                                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL))
        end = time.monotonic() + 5
        while not _owns(session, "org.a11y.Bus"):
            if time.monotonic() >= end:
                raise RuntimeError("Private accessibility launcher did not start")
            time.sleep(.05)
        reply = subprocess.run(["gdbus", "call", "--address", session, "--dest", "org.a11y.Bus",
                                "--object-path", "/org/a11y/bus", "--method", "org.a11y.Bus.GetAddress"],
                               text=True, capture_output=True, check=True, timeout=2)
        address = ast.literal_eval(reply.stdout)[0]
        if not address.startswith(f"unix:path={runtime}/"):
            raise RuntimeError("Refusing a foreign accessibility bus")
        os.environ["AT_SPI_BUS_ADDRESS"] = address
        children.append(subprocess.Popen([_executable("at-spi2-registryd")],
                                         env=dict(os.environ, DBUS_SESSION_BUS_ADDRESS=address),
                                         stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL))
        while not _owns(address, "org.a11y.atspi.Registry"):
            if time.monotonic() >= end:
                raise RuntimeError("Private accessibility registry did not start")
            time.sleep(.05)
        subprocess.run(["gdbus", "call", "--address", session, "--dest", "org.a11y.Bus",
                        "--object-path", "/org/a11y/bus", "--method", "org.freedesktop.DBus.Properties.Set",
                        "org.a11y.Status", "IsEnabled", "<true>"],
                       capture_output=True, check=True, timeout=2)
        yield
    finally:
        for child in reversed(children):
            if child.poll() is None:
                child.terminate()
            try:
                child.wait(timeout=3)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait(timeout=3)


def check_lists(expected, process_id, descriptions=None, tree_items=None):
    """Call off the GTK thread: AT-SPI requests must be served by that thread."""
    import gi
    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi
    expected = [] if expected is None else expected
    descriptions = [] if descriptions is None else descriptions
    tree_items = [] if tree_items is None else tree_items
    if (not isinstance(expected, list) or len(expected) > 8
            or not isinstance(descriptions, list) or len(descriptions) > 16
            or not isinstance(tree_items, list) or len(tree_items) > 100 or not (expected or descriptions or tree_items)):
        raise ValueError("Expected bounded accessibility lists or descriptions")
    if not all(isinstance(name, str) and 0 < len(name) <= 500 for name in tree_items):
        raise ValueError("Malformed accessibility tree item expectation")
    for entry in expected:
        if (not isinstance(entry, dict) or not isinstance(entry.get("name"), str) or len(entry["name"]) > 160
                or not isinstance(entry.get("buttons"), list) or len(entry["buttons"]) > 100
                or not all(isinstance(label, str) and len(label) <= 160 for label in entry["buttons"])):
            raise ValueError("Malformed accessibility list expectation")
    for entry in descriptions:
        if (not isinstance(entry, dict) or not isinstance(entry.get("name"), str) or len(entry["name"]) > 160
                or not isinstance(entry.get("description"), str) or len(entry["description"]) > 2000):
            raise ValueError("Malformed accessibility description expectation")
    described_names = {entry["name"] for entry in descriptions}
    Atspi.set_timeout(250, 500)
    end = time.monotonic() + 5
    observed = []
    observed_descriptions = []
    observed_tree_items = []
    error = ""
    while time.monotonic() < end:
        nodes = []

        def description(node):
            pieces = []
            for relation in node.get_relation_set()[:16]:
                if relation.get_relation_type() != Atspi.RelationType.DESCRIBED_BY:
                    continue
                for index in range(min(relation.get_n_targets(), 8)):
                    if time.monotonic() >= end:
                        break
                    target = relation.get_target(index)
                    text = target.get_text_iface()
                    pieces.append(Atspi.Text.get_text(text, 0, min(Atspi.Text.get_character_count(text), 2000)) if text else target.get_name())
            return " ".join(piece for piece in pieces if piece) or node.get_description()

        def visit(node, depth):
            if node is None or depth > 24 or len(nodes) >= 512 or time.monotonic() >= end:
                return
            name = node.get_name() or ""
            nodes.append((depth, node.get_role(), name, description(node) if name in described_names else ""))
            for index in range(min(node.get_child_count(), 512 - len(nodes))):
                if len(nodes) >= 512 or time.monotonic() >= end:
                    break
                visit(node.get_child_at_index(index), depth + 1)

        try:
            desktop = Atspi.get_desktop(0)
            for index in range(min(desktop.get_child_count(), 16)):
                app = desktop.get_child_at_index(index)
                if app is not None and app.get_process_id() == process_id:
                    visit(app, 0)
            observed = []
            for index, (depth, role, name, _) in enumerate(nodes):
                if role != Atspi.Role.LIST:
                    continue
                items = []
                for child in range(index + 1, len(nodes)):
                    child_depth, child_role = nodes[child][:2]
                    if child_depth <= depth:
                        break
                    if child_depth == depth + 1 and child_role == Atspi.Role.LIST_ITEM:
                        buttons = []
                        for nested in nodes[child + 1:]:
                            if nested[0] <= child_depth:
                                break
                            if nested[1] == Atspi.Role.PUSH_BUTTON:
                                buttons.append(nested[2][:160])
                        items.append(buttons)
                observed.append({"name": name, "items": items})
            observed_descriptions = [{"name": name, "description": description} for _, _, name, description in nodes if name in described_names]
            observed_tree_items = [name for _, role, name, _ in nodes if role == Atspi.Role.TREE_ITEM]
            if (all(any(row["name"] == entry["name"] and len(row["items"]) == len(entry["buttons"])
                       and all(len(buttons) == 1 and buttons[0].startswith(label)
                               for buttons, label in zip(row["items"], entry["buttons"]))
                       for row in observed) for entry in expected)
                    and all(entry in observed_descriptions for entry in descriptions)
                    and all(name in observed_tree_items for name in tree_items)):
                return {"accessibility": observed, "native_descriptions": observed_descriptions,
                        "native_tree_items": observed_tree_items, "accessibility_nodes": len(nodes)}
        except Exception as failure:
            error = str(failure)
        time.sleep(.1)
    mismatch = ("Native tree item names did not match" if tree_items else
                "Native accessibility description did not match" if descriptions else "Native list/item/button structure did not match")
    return {"accessibility": observed, "native_descriptions": observed_descriptions,
            "native_tree_items": observed_tree_items, "accessibility_failure": error or mismatch}
