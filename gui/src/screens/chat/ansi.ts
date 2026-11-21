/** Terminal colour codes (CSI/OSC sequences) that must not reach the plain-text output view. */
const OSC = /\u001b\][^\u0007]*(?:\u0007|\u001b\\)/g;
const CSI =
  /[\u001b\u009b][[\]()#;?]*(?:(?:(?:[a-zA-Z\d]*(?:;[-a-zA-Z\d/#&.:=?%@~_]+)*)?\u0007)|(?:(?:\d{1,4}(?:[;:]\d{0,4})*)?[\dA-PR-TZcf-nq-uy=><~]))/g;

/** Text as a terminal would show it, with colour and cursor-control sequences removed. */
export function stripAnsi(text: string): string {
  return text.replace(OSC, "").replace(CSI, "");
}
