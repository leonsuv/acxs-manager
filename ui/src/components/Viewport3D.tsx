import { useEffect, useRef, useState } from "react";
import * as THREE from "three";
import { Box, Grid, RotateCcw, RotateCw } from "lucide-react";
import type { Scene3DMeshData, Track3DSceneData } from "../lib/types";

export interface Viewport3DProps {
  mode?: "car" | "track";
  meshData?: Scene3DMeshData | null;
  trackData?: Track3DSceneData | null;
  accentHex?: string;
  label?: string;
  className?: string;
}

interface OrbitState {
  theta: number;
  phi: number;
  radius: number;
  target: THREE.Vector3;
  dragging: "none" | "orbit" | "pan";
  lastX: number;
  lastY: number;
}

function buildProceduralGtCar(accentColor: number): THREE.Group {
  const group = new THREE.Group();

  const bodyMat = new THREE.MeshStandardMaterial({
    color: 0x1a2230,
    metalness: 0.72,
    roughness: 0.24,
  });
  const carbonMat = new THREE.MeshStandardMaterial({
    color: 0x0c0f15,
    metalness: 0.45,
    roughness: 0.42,
  });
  const glassMat = new THREE.MeshStandardMaterial({
    color: 0x0a1320,
    metalness: 0.9,
    roughness: 0.08,
  });
  const accentMat = new THREE.MeshStandardMaterial({
    color: accentColor,
    emissive: accentColor,
    emissiveIntensity: 0.35,
    metalness: 0.4,
    roughness: 0.25,
  });
  const wheelMat = new THREE.MeshStandardMaterial({
    color: 0x101318,
    metalness: 0.25,
    roughness: 0.85,
  });
  const rimMat = new THREE.MeshStandardMaterial({
    color: 0x9aa4b4,
    metalness: 0.88,
    roughness: 0.2,
  });

  // Lower chassis / monocoque
  const chassis = new THREE.Mesh(new THREE.BoxGeometry(1.94, 0.42, 4.45), bodyMat);
  chassis.position.set(0, 0.46, 0);
  chassis.castShadow = true;
  group.add(chassis);

  // Sculpted front nose slope
  const nose = new THREE.Mesh(new THREE.BoxGeometry(1.86, 0.24, 1.35), bodyMat);
  nose.position.set(0, 0.48, 1.65);
  nose.rotation.x = 0.14;
  group.add(nose);

  // Front carbon splitter
  const splitter = new THREE.Mesh(new THREE.BoxGeometry(1.98, 0.05, 0.65), carbonMat);
  splitter.position.set(0, 0.21, 2.15);
  group.add(splitter);

  // Cabin canopy
  const cabin = new THREE.Mesh(new THREE.BoxGeometry(1.42, 0.42, 1.85), glassMat);
  cabin.position.set(0, 0.86, -0.1);
  group.add(cabin);

  // Roofline cap
  const roof = new THREE.Mesh(new THREE.BoxGeometry(1.32, 0.08, 1.2), bodyMat);
  roof.position.set(0, 1.07, -0.18);
  group.add(roof);

  // Side aero skirts + accent stripe
  for (const side of [-1, 1]) {
    const skirt = new THREE.Mesh(new THREE.BoxGeometry(0.12, 0.12, 2.5), carbonMat);
    skirt.position.set(side * 0.96, 0.25, 0);
    group.add(skirt);

    const stripe = new THREE.Mesh(new THREE.BoxGeometry(0.03, 0.05, 2.3), accentMat);
    stripe.position.set(side * 0.99, 0.48, 0);
    group.add(stripe);
  }

  // GT3 Rear Wing
  const wingBlade = new THREE.Mesh(new THREE.BoxGeometry(1.92, 0.05, 0.34), carbonMat);
  wingBlade.position.set(0, 1.12, -2.14);
  wingBlade.rotation.x = -0.12;
  group.add(wingBlade);

  for (const side of [-1, 1]) {
    const strut = new THREE.Mesh(new THREE.BoxGeometry(0.05, 0.48, 0.24), carbonMat);
    strut.position.set(side * 0.56, 0.88, -2.08);
    group.add(strut);

    const endplate = new THREE.Mesh(new THREE.BoxGeometry(0.04, 0.28, 0.44), accentMat);
    endplate.position.set(side * 0.96, 1.08, -2.14);
    group.add(endplate);
  }

  // Glowing LED Headlights & Taillight bar
  const headMat = new THREE.MeshStandardMaterial({
    color: 0xe8f7ff,
    emissive: 0x9de6ff,
    emissiveIntensity: 1.8,
  });
  for (const side of [-1, 1]) {
    const hl = new THREE.Mesh(new THREE.BoxGeometry(0.38, 0.07, 0.08), headMat);
    hl.position.set(side * 0.64, 0.52, 2.24);
    group.add(hl);
  }

  const tailBar = new THREE.Mesh(new THREE.BoxGeometry(1.66, 0.06, 0.06), accentMat);
  tailBar.position.set(0, 0.62, -2.23);
  group.add(tailBar);

  // 4 Wheels + Rims
  const wheelGeo = new THREE.CylinderGeometry(0.34, 0.34, 0.28, 24);
  wheelGeo.rotateZ(Math.PI / 2);
  const rimGeo = new THREE.CylinderGeometry(0.22, 0.22, 0.29, 12);
  rimGeo.rotateZ(Math.PI / 2);

  const wheelCoords: Array<[number, number, number]> = [
    [-0.94, 0.34, 1.38],
    [0.94, 0.34, 1.38],
    [-0.96, 0.34, -1.36],
    [0.96, 0.34, -1.36],
  ];
  for (const [x, y, z] of wheelCoords) {
    const tire = new THREE.Mesh(wheelGeo, wheelMat);
    tire.position.set(x, y, z);
    group.add(tire);
    const rim = new THREE.Mesh(rimGeo, rimMat);
    rim.position.set(x, y, z);
    group.add(rim);
  }

  return group;
}

function buildTrackRibbon(
  accentColor: number,
  trackData?: Track3DSceneData | null
): THREE.Group {
  const group = new THREE.Group();

  let points: THREE.Vector3[] = [];
  if (trackData?.centerline && trackData.centerline.length >= 4) {
    points = trackData.centerline.map(([x, y, z]) => new THREE.Vector3(x, y, z));
    // Normalize & center track points to ~8m span
    const box = new THREE.Box3().setFromPoints(points);
    const center = box.getCenter(new THREE.Vector3());
    const size = box.getSize(new THREE.Vector3());
    const maxDim = Math.max(size.x, size.y, size.z, 1);
    const scale = 8 / maxDim;
    points = points.map((p) =>
      p.clone().sub(center).multiplyScalar(scale).add(new THREE.Vector3(0, 0.35, 0))
    );
  } else {
    // Sleek procedural grand prix circuit loop
    const count = 96;
    for (let i = 0; i < count; i++) {
      const t = (i / count) * Math.PI * 2;
      const r = 2.6 + 0.85 * Math.sin(t * 2) + 0.45 * Math.cos(t * 3);
      const x = Math.cos(t) * r;
      const z = Math.sin(t) * (r * 0.78);
      const y = 0.25 + 0.22 * Math.sin(t * 2 + 0.6);
      points.push(new THREE.Vector3(x, y, z));
    }
  }

  const curve = new THREE.CatmullRomCurve3(points, true);
  const tubeGeo = new THREE.TubeGeometry(curve, 180, 0.085, 8, true);
  const ribbonMat = new THREE.MeshStandardMaterial({
    color: accentColor,
    emissive: accentColor,
    emissiveIntensity: 0.75,
    roughness: 0.3,
    metalness: 0.4,
  });
  const tube = new THREE.Mesh(tubeGeo, ribbonMat);
  group.add(tube);

  // Subtle asphalt halo underneath
  const haloGeo = new THREE.TubeGeometry(curve, 180, 0.19, 6, true);
  const haloMat = new THREE.MeshStandardMaterial({
    color: 0x162030,
    roughness: 0.7,
    metalness: 0.2,
    transparent: true,
    opacity: 0.55,
  });
  const halo = new THREE.Mesh(haloGeo, haloMat);
  halo.position.y = -0.06;
  group.add(halo);

  return group;
}

function buildKn5Mesh(meshData: Scene3DMeshData): THREE.Group {
  const group = new THREE.Group();
  const geo = new THREE.BufferGeometry();
  geo.setAttribute(
    "position",
    new THREE.Float32BufferAttribute(meshData.positions, 3)
  );
  if (meshData.normals && meshData.normals.length === meshData.positions.length) {
    geo.setAttribute(
      "normal",
      new THREE.Float32BufferAttribute(meshData.normals, 3)
    );
  } else {
    geo.computeVertexNormals();
  }
  if (meshData.indices && meshData.indices.length > 0) {
    geo.setIndex(meshData.indices);
  }

  geo.computeBoundingBox();
  const box = geo.boundingBox;
  if (box) {
    const center = box.getCenter(new THREE.Vector3());
    const size = box.getSize(new THREE.Vector3());
    const maxDim = Math.max(size.x, size.y, size.z, 0.001);
    const scale = 4.6 / maxDim;
    geo.translate(-center.x, -box.min.y, -center.z);
    geo.scale(scale, scale, scale);
  }

  const mat = new THREE.MeshStandardMaterial({
    color: 0xcbd5e1,
    metalness: 0.62,
    roughness: 0.28,
    side: THREE.DoubleSide,
  });

  const mesh = new THREE.Mesh(geo, mat);
  mesh.castShadow = true;
  mesh.receiveShadow = true;
  group.add(mesh);
  return group;
}

export function Viewport3D({
  mode = "car",
  meshData,
  trackData,
  accentHex = "#FF4D2E",
  label,
  className = "",
}: Viewport3DProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [wireframe, setWireframe] = useState(false);
  const [autoRotate, setAutoRotate] = useState(true);

  const wireframeRef = useRef(wireframe);
  const autoRotateRef = useRef(autoRotate);
  const orbitRef = useRef<OrbitState>({
    theta: 0.72,
    phi: 1.18,
    radius: mode === "track" ? 8.2 : 5.8,
    target: new THREE.Vector3(0, 0.55, 0),
    dragging: "none",
    lastX: 0,
    lastY: 0,
  });
  const subjectGroupRef = useRef<THREE.Group | null>(null);

  useEffect(() => {
    wireframeRef.current = wireframe;
    const grp = subjectGroupRef.current;
    if (grp) {
      grp.traverse((obj) => {
        if (obj instanceof THREE.Mesh) {
          const mat = obj.material;
          if (Array.isArray(mat)) {
            mat.forEach((m) => {
              if ("wireframe" in m) (m as THREE.MeshStandardMaterial).wireframe = wireframe;
            });
          } else if (mat && "wireframe" in mat) {
            (mat as THREE.MeshStandardMaterial).wireframe = wireframe;
          }
        }
      });
    }
  }, [wireframe]);

  useEffect(() => {
    autoRotateRef.current = autoRotate;
  }, [autoRotate]);

  const resetCamera = () => {
    orbitRef.current.theta = 0.72;
    orbitRef.current.phi = 1.18;
    orbitRef.current.radius = mode === "track" ? 8.2 : 5.8;
    orbitRef.current.target.set(0, 0.55, 0);
  };

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    const width = Math.max(container.clientWidth, 1);
    const height = Math.max(container.clientHeight, 1);

    const scene = new THREE.Scene();
    scene.background = new THREE.Color(0x070a10);
    scene.fog = new THREE.FogExp2(0x070a10, 0.042);

    const camera = new THREE.PerspectiveCamera(40, width / height, 0.1, 150);

    const renderer = new THREE.WebGLRenderer({
      antialias: true,
      alpha: false,
      powerPreference: "high-performance",
    });
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    renderer.setSize(width, height);
    renderer.toneMapping = THREE.ACESFilmicToneMapping;
    renderer.toneMappingExposure = 1.15;
    renderer.outputColorSpace = THREE.SRGBColorSpace;
    container.appendChild(renderer.domElement);

    // Studio 3-point lighting
    const hemi = new THREE.HemisphereLight(0xdbeafe, 0x0b101b, 0.9);
    scene.add(hemi);

    const keyLight = new THREE.DirectionalLight(0xffffff, 2.2);
    keyLight.position.set(6, 9, 7);
    scene.add(keyLight);

    const rimLight = new THREE.DirectionalLight(0x38bdf8, 1.35);
    rimLight.position.set(-7, 5, -6);
    scene.add(rimLight);

    const accentColorInt = new THREE.Color(accentHex).getHex();
    const fillLight = new THREE.PointLight(accentColorInt, 1.4, 18);
    fillLight.position.set(4, 2.2, -4);
    scene.add(fillLight);

    // Reflective obsidian studio floor + grid
    const floorGeo = new THREE.PlaneGeometry(40, 40);
    const floorMat = new THREE.MeshStandardMaterial({
      color: 0x090d15,
      metalness: 0.78,
      roughness: 0.34,
    });
    const floor = new THREE.Mesh(floorGeo, floorMat);
    floor.rotation.x = -Math.PI / 2;
    floor.position.y = -0.005;
    scene.add(floor);

    const grid = new THREE.GridHelper(24, 32, 0x263246, 0x131b28);
    grid.position.y = 0.001;
    scene.add(grid);

    // Subject mesh: real KN5, Track ribbon, or procedural GT car
    let subject: THREE.Group;
    if (meshData && meshData.positions && meshData.positions.length >= 9) {
      subject = buildKn5Mesh(meshData);
    } else if (mode === "track" || (trackData?.centerline && trackData.centerline.length >= 4)) {
      subject = buildTrackRibbon(accentColorInt, trackData);
    } else {
      subject = buildProceduralGtCar(accentColorInt);
    }

    // Apply initial wireframe state if enabled
    if (wireframeRef.current) {
      subject.traverse((obj) => {
        if (obj instanceof THREE.Mesh && obj.material && "wireframe" in obj.material) {
          (obj.material as THREE.MeshStandardMaterial).wireframe = true;
        }
      });
    }
    subjectGroupRef.current = subject;
    scene.add(subject);

    // Pointer controls (orbit / pan / zoom)
    const dom = renderer.domElement;
    const onPointerDown = (e: PointerEvent) => {
      dom.setPointerCapture(e.pointerId);
      orbitRef.current.dragging =
        e.button === 2 || e.shiftKey ? "pan" : "orbit";
      orbitRef.current.lastX = e.clientX;
      orbitRef.current.lastY = e.clientY;
    };

    const onPointerMove = (e: PointerEvent) => {
      const o = orbitRef.current;
      if (o.dragging === "none") return;
      const dx = e.clientX - o.lastX;
      const dy = e.clientY - o.lastY;
      o.lastX = e.clientX;
      o.lastY = e.clientY;

      if (o.dragging === "orbit") {
        o.theta -= dx * 0.007;
        o.phi = Math.max(0.22, Math.min(Math.PI * 0.48, o.phi - dy * 0.007));
      } else if (o.dragging === "pan") {
        const panSpeed = o.radius * 0.0015;
        const right = new THREE.Vector3(Math.cos(o.theta), 0, -Math.sin(o.theta));
        o.target.addScaledVector(right, -dx * panSpeed);
        o.target.y = Math.max(-1, Math.min(4, o.target.y + dy * panSpeed));
      }
    };

    const onPointerUp = (e: PointerEvent) => {
      if (dom.hasPointerCapture(e.pointerId)) {
        dom.releasePointerCapture(e.pointerId);
      }
      orbitRef.current.dragging = "none";
    };

    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      orbitRef.current.radius = Math.max(
        2.2,
        Math.min(20, orbitRef.current.radius * (1 + e.deltaY * 0.001))
      );
    };

    const onContextMenu = (e: MouseEvent) => e.preventDefault();

    dom.addEventListener("pointerdown", onPointerDown);
    dom.addEventListener("pointermove", onPointerMove);
    dom.addEventListener("pointerup", onPointerUp);
    dom.addEventListener("wheel", onWheel, { passive: false });
    dom.addEventListener("contextmenu", onContextMenu);

    // ResizeObserver for crisp responsive viewport
    const resizeObserver = new ResizeObserver(() => {
      const w = Math.max(container.clientWidth, 1);
      const h = Math.max(container.clientHeight, 1);
      camera.aspect = w / h;
      camera.updateProjectionMatrix();
      renderer.setSize(w, h);
    });
    resizeObserver.observe(container);

    // Animation loop
    let rafId = 0;
    const animate = () => {
      rafId = window.requestAnimationFrame(animate);
      const o = orbitRef.current;
      if (autoRotateRef.current && o.dragging === "none") {
        o.theta += 0.0032;
      }

      const sinPhi = Math.sin(o.phi);
      camera.position.set(
        o.target.x + o.radius * sinPhi * Math.sin(o.theta),
        o.target.y + o.radius * Math.cos(o.phi),
        o.target.z + o.radius * sinPhi * Math.cos(o.theta)
      );
      camera.lookAt(o.target);
      renderer.render(scene, camera);
    };
    animate();

    return () => {
      window.cancelAnimationFrame(rafId);
      resizeObserver.disconnect();
      dom.removeEventListener("pointerdown", onPointerDown);
      dom.removeEventListener("pointermove", onPointerMove);
      dom.removeEventListener("pointerup", onPointerUp);
      dom.removeEventListener("wheel", onWheel);
      dom.removeEventListener("contextmenu", onContextMenu);

      subjectGroupRef.current = null;
      scene.traverse((obj) => {
        if (obj instanceof THREE.Mesh) {
          obj.geometry.dispose();
          const mat = obj.material;
          if (Array.isArray(mat)) {
            mat.forEach((m) => m.dispose());
          } else if (mat) {
            mat.dispose();
          }
        }
      });
      renderer.dispose();
      if (dom.parentNode === container) {
        container.removeChild(dom);
      }
    };
  }, [mode, meshData, trackData, accentHex]);

  const triCount =
    meshData?.triangleCount ??
    (meshData?.indices?.length
      ? Math.floor(meshData.indices.length / 3)
      : meshData?.positions?.length
      ? Math.floor(meshData.positions.length / 9)
      : null);

  return (
    <div
      className={`relative w-full h-full min-h-[280px] rounded-2xl overflow-hidden border border-white/[0.08] bg-[#070A10] select-none ${className}`}
    >
      {/* WebGL Canvas Mount */}
      <div
        ref={containerRef}
        onDoubleClick={resetCamera}
        className="w-full h-full cursor-grab active:cursor-grabbing"
      />

      {/* Top-left Label / Scene Telemetry Badge */}
      <div className="absolute top-3 left-3 flex items-center gap-2 pointer-events-none">
        <span className="px-2.5 py-1 rounded-lg bg-black/55 backdrop-blur-md border border-white/[0.09] text-[11px] font-mono text-[var(--text-secondary)]">
          {label ?? (meshData ? "KN5 3D MESH" : mode === "track" ? "TRACK RIBBON" : "STUDIO PREVIEW")}
        </span>
        {triCount !== null && (
          <span className="px-2 py-1 rounded-lg bg-black/55 backdrop-blur-md border border-white/[0.08] text-[10px] font-mono text-[var(--text-muted)] tabular-nums">
            {triCount.toLocaleString()} tris
          </span>
        )}
      </div>

      {/* Top-right Floating Toolbar Pills */}
      <div className="absolute top-3 right-3 flex items-center gap-1 p-1 rounded-xl bg-black/55 backdrop-blur-md border border-white/[0.09]">
        <button
          type="button"
          onClick={() => setWireframe(false)}
          className={`h-7 px-2.5 rounded-lg text-[11px] font-medium flex items-center gap-1.5 transition-colors cursor-pointer ${
            !wireframe
              ? "bg-white/[0.12] text-white"
              : "text-[var(--text-secondary)] hover:text-white"
          }`}
        >
          <Box className="w-3 h-3" />
          <span>Shaded</span>
        </button>
        <button
          type="button"
          onClick={() => setWireframe(true)}
          className={`h-7 px-2.5 rounded-lg text-[11px] font-medium flex items-center gap-1.5 transition-colors cursor-pointer ${
            wireframe
              ? "bg-white/[0.12] text-white"
              : "text-[var(--text-secondary)] hover:text-white"
          }`}
        >
          <Grid className="w-3 h-3" />
          <span>Wireframe</span>
        </button>
        <div className="w-px h-4 bg-white/[0.1] mx-0.5" />
        <button
          type="button"
          onClick={() => setAutoRotate((v) => !v)}
          className={`h-7 px-2.5 rounded-lg text-[11px] font-medium flex items-center gap-1.5 transition-colors cursor-pointer ${
            autoRotate
              ? "bg-[var(--accent-soft)] text-[var(--accent)] border border-[var(--accent-border)]"
              : "text-[var(--text-secondary)] hover:text-white"
          }`}
        >
          <RotateCw className="w-3 h-3" />
          <span>Auto-Rotate</span>
        </button>
        <button
          type="button"
          onClick={resetCamera}
          title="Reset camera (double-click)"
          className="h-7 w-7 rounded-lg flex items-center justify-center text-[var(--text-secondary)] hover:text-white hover:bg-white/[0.08] transition-colors cursor-pointer"
        >
          <RotateCcw className="w-3 h-3" />
        </button>
      </div>

      {/* Bottom-left Control Hint */}
      <div className="absolute bottom-2.5 left-3 text-[10px] font-mono text-white/35 pointer-events-none">
        Drag to Orbit · Shift/Right-Drag to Pan · Scroll to Zoom
      </div>
    </div>
  );
}
