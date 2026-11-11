import { createRoot } from "react-dom/client";
import { I18nextProvider } from "react-i18next";
import { App } from "./App";
import { createEngine } from "./i18n";
import "./styles.css";

const engine = await createEngine();
createRoot(document.getElementById("root")!).render(
  <I18nextProvider i18n={engine}>
    <App />
  </I18nextProvider>,
);
