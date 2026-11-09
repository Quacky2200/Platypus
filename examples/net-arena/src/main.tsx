// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// Net arena: the multiplayer testbed. A clinical white town in white fog with red lasers, a laser
// gun, a match (spawn, die, kills, a session scoreboard) and a debug panel that makes the network
// bad on purpose. Run it as a dedicated server (`-- --server`, with `--headless` for none of the
// graphics) and join from other windows (`-- --connect 127.0.0.1:7777`), or use the menu.
//
// Keys: WASD move, Shift sprint, Space jump, left mouse or F fire, the locked mouse (Esc frees it) or Q / E turns the
// camera, wheel zoom, Tab scoreboard, F1 network panel.

import "./game";
import { hud, onHud, request } from "./game";
import { KILL_LIMIT } from "./schema";

const TEXT = "#20242c";
const SOFT = "#5a6270";
const PANEL = "rgba(255, 255, 255, 0.72)";
const RED = "#d8262a";
// Only the join menu is glass: white, blurring and slightly dimming the town behind it.
const GLASS = "rgba(255, 255, 255, 0.5)";
const BLUR = "blur(18px) brightness(0.92)";

/** A text field: a clear border while it has focus. */
function Field(props: { value: string; onChange: (v: string) => void }) {
  const [focused, setFocused] = useState(false);
  return (
    <input
      value={props.value}
      onChange={(e) => props.onChange(e.target.value ?? "")}
      onFocus={() => setFocused(true)}
      onBlur={() => setFocused(false)}
      style={{
        padding: "8px 10px",
        borderRadius: 6,
        backgroundColor: "#ffffff",
        color: TEXT,
        fontSize: 14,
        width: 260,
        borderWidth: 2,
        borderStyle: "solid",
        borderColor: focused ? RED : "rgba(32, 36, 44, 0.18)",
      }}
    />
  );
}

function Button(props: { label: string; onClick: () => void; primary?: boolean; small?: boolean }) {
  return (
    <box
      onClick={props.onClick}
      style={{
        padding: props.small ? "4px 10px" : "10px 22px",
        borderRadius: 8,
        backgroundColor: props.primary ? RED : "rgba(32, 36, 44, 0.08)",
        cursor: "pointer",
        alignItems: "center",
      }}
    >
      <text style={{ color: props.primary ? "#ffffff" : TEXT, fontSize: props.small ? 12 : 15, fontWeight: 600 }}>{props.label}</text>
    </box>
  );
}

function Menu() {
  const [name, setName] = useState(request.name);
  const [address, setAddress] = useState(request.address);
  const go = (mode: "server" | "client") => {
    request.name = name.trim() || "Player";
    request.address = address.trim() || "127.0.0.1:7777";
    request.mode = mode;
  };
  return (
    <box style={{ width: "100%", height: "100%", alignItems: "center", justifyContent: "center" }}>
      <box style={{ padding: 28, gap: 12, borderRadius: 14, backgroundColor: GLASS, backdropFilter: BLUR, flexDirection: "column", alignItems: "stretch" }}>
        <text style={{ color: TEXT, fontSize: 26, fontWeight: 700 }}>Net arena</text>
        <text style={{ color: SOFT, fontSize: 13 }}>A white town, red lasers, and a network you can break.</text>
        <text style={{ color: SOFT, fontSize: 12, marginTop: 8 }}>NAME</text>
        <Field value={name} onChange={setName} />
        <text style={{ color: SOFT, fontSize: 12 }}>SERVER</text>
        <Field value={address} onChange={setAddress} />
        <box style={{ flexDirection: "row", gap: 10, marginTop: 8 }}>
          <Button primary label="Join" onClick={() => go("client")} />
          <Button label="Host a server here" onClick={() => go("server")} />
        </box>
        {hud.error !== "" && <text style={{ color: RED, fontSize: 13, maxWidth: 300 }}>{hud.error}</text>}
        <text style={{ color: SOFT, fontSize: 12, marginTop: 6 }}>
          Hosting runs the match in this window (it watches); join from other windows on this machine.
        </text>
      </box>
    </box>
  );
}

function Scoreboard() {
  return (
    <box style={{ position: "absolute", left: "50%", top: 90, marginLeft: -230, width: 460, padding: 16, gap: 4, borderRadius: 12, backgroundColor: PANEL, flexDirection: "column" }}>
      <box style={{ flexDirection: "row" }}>
        <text style={{ color: SOFT, fontSize: 12, width: 220 }}>PLAYER</text>
        <text style={{ color: SOFT, fontSize: 12, width: 70 }}>KILLS</text>
        <text style={{ color: SOFT, fontSize: 12, width: 70 }}>DEATHS</text>
        <text style={{ color: SOFT, fontSize: 12, width: 60 }}>PING</text>
      </box>
      {hud.rows.map((r, i) => (
        <box key={i} style={{ flexDirection: "row", padding: "3px 0" }}>
          <text style={{ color: r.me ? RED : TEXT, fontSize: 15, width: 220, fontWeight: r.me ? 700 : 400 }}>{r.name}</text>
          <text style={{ color: TEXT, fontSize: 15, width: 70 }}>{String(r.kills)}</text>
          <text style={{ color: TEXT, fontSize: 15, width: 70 }}>{String(r.deaths)}</text>
          <text style={{ color: SOFT, fontSize: 15, width: 60 }}>{String(r.ping)}</text>
        </box>
      ))}
      {hud.rows.length === 0 && <text style={{ color: SOFT, fontSize: 14 }}>Nobody yet.</text>}
    </box>
  );
}

interface Preset {
  label: string;
  latency: number;
  jitter: number;
  loss: number;
}

const PRESETS: Preset[] = [
  { label: "Clean", latency: 0, jitter: 0, loss: 0 },
  { label: "50 ms", latency: 25, jitter: 3, loss: 0 },
  { label: "150 ms", latency: 75, jitter: 15, loss: 1 },
  { label: "300 ms", latency: 150, jitter: 40, loss: 3 },
  { label: "Bad wifi", latency: 60, jitter: 70, loss: 12 },
  { label: "Awful", latency: 250, jitter: 120, loss: 25 },
];

function DebugPanel() {
  const [set, setSet] = useState("Clean");
  const apply = (p: Preset) => {
    setSet(p.label);
    // The conditions are per direction: half of the round trip each way.
    Network.debugConditions(p.latency, p.jitter, p.loss);
  };
  return (
    <box style={{ position: "absolute", right: 16, bottom: 16, width: 330, padding: 14, gap: 8, borderRadius: 12, backgroundColor: PANEL, flexDirection: "column" }}>
      <text style={{ color: TEXT, fontSize: 14, fontWeight: 700 }}>Network (F1)</text>
      <text style={{ color: SOFT, fontSize: 12 }}>{`ping ${Math.round(hud.rtt)} ms · players ${hud.players} · refused rpcs ${hud.dropped}`}</text>
      {!Network.hasDebug() && <text style={{ color: SOFT, fontSize: 12 }}>This build cannot inject faults.</text>}
      <box style={{ flexDirection: "row", gap: 6, flexWrap: "wrap" }}>
        {PRESETS.map((p) => (
          <Button key={p.label} small primary={set === p.label} label={p.label} onClick={() => apply(p)} />
        ))}
      </box>
      <box style={{ flexDirection: "row", gap: 6, flexWrap: "wrap" }}>
        <Button small label="Freeze 1 s" onClick={() => Network.debugFault("freeze:1000")} />
        <Button small label="Blackhole" onClick={() => Network.debugFault("blackhole:on")} />
        <Button small label="Restore" onClick={() => Network.debugFault("blackhole:off")} />
        <Button small label="Drop" onClick={() => Network.debugFault("disconnect")} />
      </box>
    </box>
  );
}

function Hud() {
  const [, setTick] = useState(0);
  useEffect(() => {
    onHud(() => setTick((t) => t + 1));
  }, []);
  if (hud.mode === "menu") return <Menu />;
  const matchText =
    hud.match === "waiting" ? "Waiting for another player"
    : hud.match === "roundEnd" ? `${hud.winner || "Nobody"} wins the round`
    : `Round ${hud.round}  ·  first to ${KILL_LIMIT}  ·  ${Math.ceil(hud.timeLeft)} s`;
  const hurt = Time.time < hud.hurt;
  return (
    <box style={{ width: "100%", height: "100%", pointerEvents: "none" }}>
      {hurt && <box style={{ position: "absolute", left: 0, top: 0, width: "100%", height: "100%", backgroundColor: "rgba(216, 38, 42, 0.22)" }} />}
      <box style={{ position: "absolute", left: 16, top: 16, padding: "8px 12px", borderRadius: 8, backgroundColor: PANEL, flexDirection: "column", gap: 2 }}>
        <text style={{ color: TEXT, fontSize: 14, fontWeight: 600 }}>{hud.mode === "server" ? "Server" : "Client"}</text>
        {hud.status !== "" && <text style={{ color: SOFT, fontSize: 12 }}>{hud.status}</text>}
        <text style={{ color: SOFT, fontSize: 12 }}>{`ping ${hud.rtt < 0 ? "-" : Math.round(hud.rtt)} ms`}</text>
      </box>
      <box style={{ position: "absolute", left: "50%", top: 16, marginLeft: -170, width: 340, padding: "8px 12px", borderRadius: 8, backgroundColor: PANEL, alignItems: "center" }}>
        <text style={{ color: TEXT, fontSize: 15, fontWeight: 600 }}>{matchText}</text>
      </box>
      <box style={{ position: "absolute", right: 16, top: 16, flexDirection: "column", gap: 4, alignItems: "flex-end" }}>
        {hud.feed.map((f, i) => (
          <box key={i} style={{ padding: "4px 10px", borderRadius: 6, backgroundColor: PANEL }}>
            <text style={{ color: TEXT, fontSize: 14 }}>{f.text}</text>
          </box>
        ))}
      </box>
      {hud.mode === "client" && (
        <box style={{ position: "absolute", left: 16, bottom: 16, width: 220, padding: 10, borderRadius: 10, backgroundColor: PANEL, flexDirection: "column", gap: 6 }}>
          <text style={{ color: TEXT, fontSize: 13 }}>{hud.alive ? (hud.shield ? "Shielded" : "Health") : "Respawning..."}</text>
          <box style={{ width: "100%", height: 10, borderRadius: 5, backgroundColor: "rgba(32, 36, 44, 0.15)" }}>
            <box style={{ width: `${Math.max(0, Math.min(100, hud.hp))}%`, height: 10, borderRadius: 5, backgroundColor: hud.shield ? "#4b8cff" : RED }} />
          </box>
        </box>
      )}
      {hud.mode === "client" && hud.alive && (
        <box style={{ position: "absolute", left: "50%", top: "50%", width: 6, height: 6, marginLeft: -3, marginTop: -3, borderRadius: 3, backgroundColor: RED }} />
      )}
      {(hud.showScores || hud.match === "roundEnd") && <Scoreboard />}
      {hud.debug && <box style={{ pointerEvents: "auto" }}><DebugPanel /></box>}
    </box>
  );
}

createRoot().render(<Hud />);
