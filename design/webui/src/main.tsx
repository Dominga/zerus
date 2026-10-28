import { render } from "solid-js/web";
import "./styles/base.css";
import "./styles/app.css";
import "./styles/sessions.css";
import "./styles/detail.css";
import "./styles/screens.css";
import "./skin/runtime";
import { App } from "./App";

render(() => <App />, document.getElementById("root")!);
