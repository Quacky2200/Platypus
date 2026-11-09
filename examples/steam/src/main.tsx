// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// The `Steam` namespace: rich presence, an achievement, and a lobby you create and search for.
// Nothing runs until you press a button, and Steam stays off until `project.json` has your own app
// id: add  "steam": { "appId": 123456 }  (never another game's id: lobbies and searches would
// meet that game's real players). Without Steam every call does nothing.

const ACHIEVEMENT = "ACH_EXAMPLE";

function Button(props: { label: string; onClick: () => void }) {
  return (
    <box
      onClick={props.onClick}
      style={{ padding: 10, margin: 4, backgroundColor: "#335", borderRadius: 6, cursor: "pointer" }}
    >
      <text style={{ color: "white", fontSize: 16 }}>{props.label}</text>
    </box>
  );
}

function Line(props: { text: string }) {
  return <text style={{ color: "#ccd", fontSize: 16, margin: 2 }}>{props.text}</text>;
}

function App() {
  const [tick, setTick] = useState(0);
  const [note, setNote] = useState("");

  useEffect(() => {
    const id = setInterval(() => setTick((t) => t + 1), 250);
    return () => clearInterval(id);
  }, []);
  void tick;

  const lobby = JSON.parse(Steam.lobby());
  const found = JSON.parse(Steam.lobbies());

  return (
    <box style={{ padding: 24, backgroundColor: "#112", width: "100%", height: "100%", flexDirection: "column" }}>
      <text style={{ color: "white", fontSize: 24, marginBottom: 12 }}>Steam</text>
      <Line
        text={
          Steam.isAvailable()
            ? `connected as app ${Steam.appId()}, player ${Steam.userName()} (${Steam.userId()})`
            : 'Steam is off: add "steam": { "appId": <your app id> } to project.json'
        }
      />
      <Line text={`${ACHIEVEMENT}: ${Steam.isAchievementUnlocked(ACHIEVEMENT) ? "unlocked" : "locked"}`} />
      <Line text={`lobby: ${Steam.lobbyStatus()} ${lobby.id} members: ${lobby.members.length} ${lobby.error}`} />
      <Line text={`search: ${Steam.searchStatus()} found: ${found.length}`} />
      <Line text={note} />
      <box style={{ flexDirection: "row", marginTop: 12 }}>
        <Button
          label="Set presence"
          onClick={() => setNote(`presence: ${Steam.setRichPresence("status", "Pressed a button")}`)}
        />
        <Button
          label="Unlock achievement"
          onClick={() => setNote(`unlock ${ACHIEVEMENT}: ${Steam.unlockAchievement(ACHIEVEMENT)}`)}
        />
        <Button
          label="Create private lobby"
          onClick={() => {
            Steam.createLobby("private", 4);
            setNote("creating a lobby");
          }}
        />
        <Button
          label="Find lobbies"
          onClick={() => {
            Steam.findLobbies();
            setNote("searching");
          }}
        />
        <Button label="Leave lobby" onClick={() => Steam.leaveLobby()} />
      </box>
    </box>
  );
}

createRoot().render(<App />);
