import React from "react";
import ReactDOM from "react-dom/client";
import "./index.css";

function App() {
  return <main className="p-6 text-sm">VoiceFlow</main>;
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
