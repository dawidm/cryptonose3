import { forwardRef } from "react";
import { useStore } from "../store";
import { updateToggles } from "../actions";

export const Header = forwardRef<HTMLInputElement>(function Header(_props, findRef) {
  const settings = useStore((s) => s.settings);
  const find = useStore((s) => s.find);
  const setFind = useStore((s) => s.setFind);
  const setDialog = useStore((s) => s.setDialog);
  const t = settings?.toggles;
  const toggle = (label: string, key: "sound" | "browser" | "notification" | "powerSave", shortcut: string) => (
    <button className={`toggle ${t?.[key] ? "on" : ""}`} aria-pressed={!!t?.[key]} onClick={() => updateToggles({ [key]: !t?.[key] })} title={`${label} (${shortcut})`}>
      {label}
    </button>
  );
  return (
    <header className="app-header">
      <div className="brand">
        <img src="/icon.svg" alt="" width={22} height={22} />
        <span>Cryptonose 3</span>
      </div>
      <div className="toggles" role="group" aria-label="Alert actions">
        {toggle("Sound", "sound", "Ctrl+S")}
        {toggle("Browser", "browser", "Ctrl+B")}
        {toggle("Notify", "notification", "Ctrl+N")}
      </div>
      {toggle("Power save", "powerSave", "Ctrl+P")}
      <span className="spacer" />
      <input
        ref={findRef}
        className="find"
        placeholder="Find pair (F)"
        value={find}
        onChange={(e) => setFind(e.target.value)}
        onKeyDown={(e) => e.key === "Escape" && (setFind(""), (e.target as HTMLInputElement).blur())}
      />
      <button onClick={() => setDialog({ kind: "settings" })} title="Settings">
        Settings
      </button>
      <button onClick={() => setDialog({ kind: "help" })} title="Help">
        ?
      </button>
    </header>
  );
});
