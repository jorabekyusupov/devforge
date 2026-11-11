import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";
afterEach(() => {
  cleanup();
  window.location.hash = "";
});
if(!HTMLDialogElement.prototype.showModal){HTMLDialogElement.prototype.showModal=function(){this.setAttribute('open','');this.querySelector<HTMLElement>('input,button')?.focus();};HTMLDialogElement.prototype.close=function(){this.removeAttribute('open');};}
