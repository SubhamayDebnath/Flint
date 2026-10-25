import { Component, type ReactNode } from "react";
import ReactDOM from "react-dom/client";
import "./index.css";
import App from "./App";

document.addEventListener("contextmenu", (e) => e.preventDefault());
// a launcher is not a browser: no reload / print / find shortcuts
document.addEventListener("keydown", (e) => {
  if (
    e.key === "F5" ||
    ((e.ctrlKey || e.metaKey) && ["p", "f", "g", "u"].includes(e.key.toLowerCase()))
  )
    e.preventDefault();
});

/** If rendering ever throws, recover once by reloading instead of leaving a dead window. */
class Boundary extends Component<{ children: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() {
    return { failed: true };
  }
  componentDidCatch() {
    const KEY = "flint-recovered";
    if (!sessionStorage.getItem(KEY)) {
      sessionStorage.setItem(KEY, "1");
      location.reload();
    }
  }
  render() {
    return this.state.failed ? null : this.props.children;
  }
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <Boundary>
    <App />
  </Boundary>,
);
