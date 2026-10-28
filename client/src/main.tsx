import { render } from "solid-js/web";
import { createSignal } from "solid-js";
import "./ui/styles/base.css";
import "./ui/styles/app.css";
import "./ui/styles/sessions.css";
import "./ui/styles/detail.css";
import "./ui/styles/client.css";
import "./ui/styles/screens.css";
import "./ui/styles/machines.css";
import "./ui/styles/motion.css";
import "./skin/runtime";
import { App } from "./App";
import { startBridge, type BridgeMode } from "./bridge";
import { receive } from "./store/session";
import { startMotion } from "./ui/motion";

const [mode, setMode] = createSignal<BridgeMode | "starting">("starting");

// The motion attributes are on the root before the first frame.
startMotion();
render(() => <App mode={mode} />, document.getElementById("root")!);

void startBridge(receive).then(setMode, (error) => console.error("Zerus Next: the data source did not start", error));
