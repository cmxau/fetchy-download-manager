import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource-variable/inter";
import "./styles.css";
import App from "./App";
import { ClipPrompt } from "./components/ClipPrompt";

// The clipboard popup is a second window loading this same bundle with ?clip=1.
const isClip = new URLSearchParams(location.search).has("clip");
// Platform hook for CSS: only the "mac" class changes anything; Windows/Linux keep the default look.
if (/Mac/.test(navigator.userAgent)) document.documentElement.classList.add("mac");
if (isClip) document.documentElement.classList.add("clip-window");

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>{isClip ? <ClipPrompt /> : <App />}</React.StrictMode>,
);
