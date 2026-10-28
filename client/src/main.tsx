import { render } from "solid-js/web";
import { createSignal } from "solid-js";
import "./ui/styles/base.css";
import "./ui/styles/app.css";
import "./ui/styles/sessions.css";
import "./ui/styles/detail.css";
import "./ui/styles/client.css";
import "./skin/runtime";
import { App } from "./App";
import { startBridge, type BridgeMode } from "./bridge";
import { receive } from "./store/session";

const [mode, setMode] = createSignal<BridgeMode | "starting">("starting");

render(() => <App mode={mode} />, document.getElementById("root")!);

void startBridge(receive).then(setMode, (error) => console.error("Zerus Next: the data source did not start", error));
