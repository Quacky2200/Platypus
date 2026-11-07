// Copyright 2026 The Platypus Authors
// SPDX-License-Identifier: Apache-2.0

// A 3D view inside a React layout: a camera draws into a `<canvas>` element
// (`Camera.targetCanvas`) at the element's size, following it as the layout changes. The
// sidebar is ordinary UI; the button widens the view.

/** The `<canvas>` the camera should draw into, once React has made it. */
let viewElement: PlatypusElement | null = null;

class Spin extends Component {
  degreesPerSecond = new Vector3(20, 45, 0);
  override onUpdate(dt: number) {
    this.transform.rotate(this.degreesPerSecond.scale(dt));
  }
}

class ViewScene extends Component {
  private camera: Camera | undefined;

  override onStart() {
    const cam = new GameObject("View Camera", { position: new Vector3(0, 1.2, -4) });
    cam.transform.lookAt(new Vector3(0, 0.4, 0));
    this.camera = cam.addComponent(Camera, { fieldOfView: 50, backgroundColor: Color.hex("#10141f") });

    const sun = new GameObject("Sun", { rotation: Quaternion.euler(-45, 30, 0) });
    sun.addComponent(Light, { type: "directional", intensity: 1.1 });

    const cube = new GameObject("Cube", { position: new Vector3(0, 0.6, 0), rotation: Quaternion.euler(25, 35, 0) });
    cube.addComponent(MeshRenderer, { mesh: "cube", material: new Material(Color.hex("#4fa3ff"), Color.black, 0.4) });
    cube.addComponent(Spin);

    const floor = new GameObject("Floor", { scale: Vector3.one.scale(0.4) });
    floor.addComponent(MeshRenderer, { mesh: "plane", material: new Material(Color.hex("#3a4152"), Color.black, 0.2) });
  }

  override onUpdate() {
    // The element exists after React's first commit; the camera then draws only there.
    if (this.camera && viewElement && this.camera.targetCanvas === undefined) {
      this.camera.targetCanvas = viewElement;
    }
  }
}

register({ ViewScene });

function App() {
  const [wide, setWide] = useState(false);
  const view = useRef<PlatypusElement>(null);
  useEffect(() => {
    viewElement = view.current;
  }, []);
  return (
    <box style={{ width: "100%", height: "100%", flexDirection: "row", backgroundColor: "#1b2030" }}>
      <box style={{ width: wide ? 160 : 280, flexShrink: 0, padding: 20, gap: 12, flexDirection: "column" }}>
        <text style={{ color: "#e8eefc", fontSize: 22, fontWeight: 700 }}>Canvas view</text>
        <text style={{ color: "#9fb0cc", fontSize: 14 }}>A camera draws into the canvas on the right, sized to its box.</text>
        <box
          style={{ alignSelf: "flex-start", padding: "8px 12px", borderRadius: 8, backgroundColor: "#2f6fd0", cursor: "pointer" }}
          onClick={() => setWide((w) => !w)}
        >
          <text style={{ color: "#ffffff", fontSize: 14 }}>{wide ? "Narrower view" : "Wider view"}</text>
        </box>
      </box>
      <canvas ref={view} style={{ flexGrow: 1, flexBasis: 0, minWidth: 0, margin: 20, borderRadius: 12 }} />
    </box>
  );
}

createRoot().render(<App />);
