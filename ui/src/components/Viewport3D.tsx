import { useEffect, useRef, useState } from "react";
import * as THREE from "three";
import { AlertTriangle, Box, Grid, RotateCcw, RotateCw } from "lucide-react";
import type { Scene3DMeshData, Track3DSceneData } from "../lib/types";

export interface Viewport3DProps {
  mode?: "car" | "track";
  meshData?: Scene3DMeshData | null;
  trackData?: Track3DSceneData | null;
  accentHex?: string;
  label?: string;
  loading?: boolean;
  className?: string;
}

function disposeSubjectGroup(
  scene: THREE.Scene,
  group: THREE.Group | null,
  textures: THREE.Texture[]
) {
  if (group) {
    scene.remove(group);
    group.traverse((obj) => {
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
  }
  textures.forEach((t) => t.dispose());
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

function buildKn5Mesh(
  meshData: Scene3DMeshData,
  maxAnisotropy: number
): { group: THREE.Group; textures: THREE.Texture[] } {
  const group = new THREE.Group();
  const createdTextures: THREE.Texture[] = [];

  if (meshData.parts && meshData.parts.length > 0) {
    const loader = new THREE.TextureLoader();
    const texMap = new Map<string, THREE.Texture>();
    if (meshData.textures) {
      for (const [key, dataUrl] of Object.entries(meshData.textures)) {
        const tex = loader.load(dataUrl);
        tex.colorSpace = THREE.SRGBColorSpace;
        tex.wrapS = THREE.RepeatWrapping;
        tex.wrapT = THREE.RepeatWrapping;
        tex.anisotropy = Math.min(8, Math.max(1, maxAnisotropy));
        tex.needsUpdate = true;
        texMap.set(key, tex);
        createdTextures.push(tex);
      }
    }

    const materials: THREE.MeshStandardMaterial[] = (meshData.materials ?? []).map((m) => {
      const shaderLow = (m.shader ?? "").toLowerCase();
      const nameLow = (m.name ?? "").toLowerCase();
      const tex = m.diffuseTexture ? texMap.get(m.diffuseTexture) ?? null : null;
      const isGlass =
        shaderLow.includes("windscreen") ||
        shaderLow.includes("glass") ||
        nameLow.includes("glass") ||
        nameLow.includes("windscreen") ||
        nameLow.includes("window");
      const isTyre =
        shaderLow.includes("tyre") ||
        shaderLow.includes("tire") ||
        nameLow.includes("tyre") ||
        nameLow.includes("tire") ||
        nameLow.includes("rubber");
      const isTransparent = m.blendMode === 1 || isGlass;
      const isPaint =
        shaderLow.includes("multimap") ||
        shaderLow.includes("reflection") ||
        shaderLow.includes("carpaint");

      let baseColor = 0xffffff;
      if (!tex) {
        if (isGlass) {
          baseColor = 0x111827;
        } else if (isTyre) {
          baseColor = 0x1c1f24;
        } else {
          const lum = Math.max(0.18, Math.min(0.88, m.ksDiffuse || 0.55));
          const c = Math.round(lum * 255);
          baseColor = (c << 16) | (c << 8) | c;
        }
      }

      const roughness = isGlass
        ? 0.08
        : isTyre
          ? 0.82
          : isPaint
            ? 0.24
            : m.ksSpecular > 0.6
              ? 0.34
              : 0.62;
      const metalness = isGlass
        ? 0.1
        : isTyre
          ? 0.04
          : isPaint
            ? 0.22
            : m.ksSpecular > 0.8
              ? 0.48
              : 0.12;

      return new THREE.MeshStandardMaterial({
        color: baseColor,
        map: tex,
        metalness,
        roughness,
        transparent: isTransparent,
        opacity: isGlass ? (tex ? 0.48 : 0.32) : 1.0,
        depthWrite: !isTransparent,
        alphaTest: m.alphaTested ? 0.38 : 0,
        side: THREE.DoubleSide,
      });
    });

    const fallbackMat = new THREE.MeshStandardMaterial({
      color: 0xcbd5e1,
      metalness: 0.45,
      roughness: 0.35,
      side: THREE.DoubleSide,
    });

    // Compute overall bounds for consistent centering/scaling across all sub-meshes.
    const box = new THREE.Box3();
    if (meshData.bounds) {
      box.min.set(...meshData.bounds.min);
      box.max.set(...meshData.bounds.max);
    } else {
      for (const part of meshData.parts) {
        for (let i = 0; i + 2 < part.positions.length; i += 3) {
          box.expandByPoint(
            new THREE.Vector3(part.positions[i], part.positions[i + 1], part.positions[i + 2])
          );
        }
      }
    }
    const center = box.getCenter(new THREE.Vector3());
    const size = box.getSize(new THREE.Vector3());
    const maxDim = Math.max(size.x, size.y, size.z, 0.001);
    const scale = 4.6 / maxDim;

    const inner = new THREE.Group();
    for (const part of meshData.parts) {
      if (!part.positions || part.positions.length < 9) continue;
      const geo = new THREE.BufferGeometry();
      geo.setAttribute("position", new THREE.Float32BufferAttribute(part.positions, 3));
      if (part.normals && part.normals.length === part.positions.length) {
        geo.setAttribute("normal", new THREE.Float32BufferAttribute(part.normals, 3));
      } else {
        geo.computeVertexNormals();
      }
      if (part.uvs && part.uvs.length === (part.positions.length / 3) * 2) {
        geo.setAttribute("uv", new THREE.Float32BufferAttribute(part.uvs, 2));
      }
      if (part.indices && part.indices.length > 0) {
        geo.setIndex(part.indices);
      }
      const mat = materials[part.materialIndex] ?? fallbackMat;
      const mesh = new THREE.Mesh(geo, mat);
      mesh.castShadow = !mat.transparent;
      mesh.receiveShadow = true;
      if (mat.transparent) {
        mesh.renderOrder = 2;
      }
      inner.add(mesh);
    }

    inner.position.set(-center.x * scale, -box.min.y * scale, -center.z * scale);
    inner.scale.set(scale, scale, scale);
    group.add(inner);
    return { group, textures: createdTextures };
  }

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
  return { group, textures: createdTextures };
}

export function Viewport3D({
  mode = "car",
  meshData,
  trackData,
  accentHex = "#FF4D2E",
  label,
  loading = false,
  className = "",
}: Viewport3DProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [wireframe, setWireframe] = useState(false);
  const [autoRotate, setAutoRotate] = useState(true);
  const [webglError, setWebglError] = useState<string | null>(null);
  const [sceneReady, setSceneReady] = useState(false);

  const wireframeRef = useRef(wireframe);
  const autoRotateRef = useRef(autoRotate);
  const modeRef = useRef(mode);
  const accentHexRef = useRef(accentHex);
  const orbitRef = useRef<OrbitState>({
    theta: 0.72,
    phi: 1.18,
    radius: mode === "track" ? 8.2 : 5.8,
    target: new THREE.Vector3(0, 0.55, 0),
    dragging: "none",
    lastX: 0,
    lastY: 0,
  });
  const sceneRef = useRef<THREE.Scene | null>(null);
  const rendererRef = useRef<THREE.WebGLRenderer | null>(null);
  const fillLightRef = useRef<THREE.PointLight | null>(null);
  const subjectGroupRef = useRef<THREE.Group | null>(null);
  const loadedTexturesRef = useRef<THREE.Texture[]>([]);

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

  useEffect(() => {
    modeRef.current = mode;
  }, [mode]);

  useEffect(() => {
    accentHexRef.current = accentHex;
  }, [accentHex]);

  const resetCamera = () => {
    orbitRef.current.theta = 0.72;
    orbitRef.current.phi = 1.18;
    orbitRef.current.radius = modeRef.current === "track" ? 8.2 : 5.8;
    orbitRef.current.target.set(0, 0.55, 0);
  };

  // Mount WebGLRenderer, scene, camera, lights, studio floor, GridHelper, and orbit loop ONCE
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    setWebglError(null);
    setSceneReady(false);
    const width = Math.max(container.clientWidth, 1);
    const height = Math.max(container.clientHeight, 1);

    const scene = new THREE.Scene();
    scene.background = new THREE.Color(0x0b1019);
    scene.fog = new THREE.FogExp2(0x0b1019, 0.026);

    const camera = new THREE.PerspectiveCamera(40, width / height, 0.1, 150);

    let renderer: THREE.WebGLRenderer;
    try {
      renderer = new THREE.WebGLRenderer({
        antialias: true,
        alpha: false,
        powerPreference: "high-performance",
      });
    } catch (err) {
      setWebglError(
        err instanceof Error
          ? err.message
          : "WebGL graphics context could not be initialized."
      );
      return;
    }

    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    renderer.setSize(width, height);
    renderer.toneMapping = THREE.ACESFilmicToneMapping;
    renderer.toneMappingExposure = 1.38;
    renderer.outputColorSpace = THREE.SRGBColorSpace;
    container.appendChild(renderer.domElement);

    // Bright studio 3-point + soft wrap lighting so dark/carbon & textured liveries read vividly
    const hemi = new THREE.HemisphereLight(0xf8fafc, 0x334155, 1.85);
    scene.add(hemi);

    const keyLight = new THREE.DirectionalLight(0xffffff, 2.8);
    keyLight.position.set(6, 9, 7);
    scene.add(keyLight);

    const rimLight = new THREE.DirectionalLight(0xbae6fd, 1.8);
    rimLight.position.set(-7, 5, -6);
    scene.add(rimLight);

    const frontFill = new THREE.DirectionalLight(0xffffff, 1.25);
    frontFill.position.set(-4, 4, 8);
    scene.add(frontFill);

    const accentColorInt = new THREE.Color(accentHexRef.current).getHex();
    const fillLight = new THREE.PointLight(accentColorInt, 1.4, 20);
    fillLight.position.set(4, 2.4, -4);
    scene.add(fillLight);

    // Studio floor + subtle illuminated grid
    const floorGeo = new THREE.PlaneGeometry(40, 40);
    const floorMat = new THREE.MeshStandardMaterial({
      color: 0x111827,
      metalness: 0.65,
      roughness: 0.32,
    });
    const floor = new THREE.Mesh(floorGeo, floorMat);
    floor.rotation.x = -Math.PI / 2;
    floor.position.y = -0.005;
    scene.add(floor);

    const grid = new THREE.GridHelper(24, 32, 0x3b4c6b, 0x1c2536);
    grid.position.y = 0.001;
    scene.add(grid);

    sceneRef.current = scene;
    rendererRef.current = renderer;
    fillLightRef.current = fillLight;

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
    let firstFrameRendered = false;
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
      if (!firstFrameRendered) {
        firstFrameRendered = true;
        setSceneReady(true);
      }
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

      disposeSubjectGroup(scene, subjectGroupRef.current, loadedTexturesRef.current);
      subjectGroupRef.current = null;
      loadedTexturesRef.current = [];
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
      sceneRef.current = null;
      rendererRef.current = null;
      fillLightRef.current = null;
      renderer.dispose();
      if (dom.parentNode === container) {
        container.removeChild(dom);
      }
    };
  }, []);

  // Swap and dispose subject mesh when [mode, meshData, trackData, accentHex] updates
  useEffect(() => {
    const scene = sceneRef.current;
    const renderer = rendererRef.current;
    if (!scene || !renderer) return;

    disposeSubjectGroup(scene, subjectGroupRef.current, loadedTexturesRef.current);
    subjectGroupRef.current = null;
    loadedTexturesRef.current = [];

    const accentColorInt = new THREE.Color(accentHex).getHex();
    if (fillLightRef.current) {
      fillLightRef.current.color.setHex(accentColorInt);
    }

    let subject: THREE.Group;
    let loadedTextures: THREE.Texture[] = [];
    const hasKn5 =
      meshData &&
      ((meshData.parts && meshData.parts.length > 0) ||
        (meshData.positions && meshData.positions.length >= 9));
    if (hasKn5 && meshData) {
      const built = buildKn5Mesh(meshData, renderer.capabilities.getMaxAnisotropy());
      subject = built.group;
      loadedTextures = built.textures;
    } else if (
      mode === "track" ||
      (trackData?.centerline && trackData.centerline.length >= 4)
    ) {
      subject = buildTrackRibbon(accentColorInt, trackData);
    } else {
      subject = buildProceduralGtCar(accentColorInt);
    }

    if (wireframeRef.current) {
      subject.traverse((obj) => {
        if (obj instanceof THREE.Mesh) {
          const mat = obj.material;
          if (Array.isArray(mat)) {
            mat.forEach((m) => {
              if ("wireframe" in m) (m as THREE.MeshStandardMaterial).wireframe = true;
            });
          } else if (mat && "wireframe" in mat) {
            (mat as THREE.MeshStandardMaterial).wireframe = true;
          }
        }
      });
    }

    subjectGroupRef.current = subject;
    loadedTexturesRef.current = loadedTextures;
    scene.add(subject);

    return () => {
      disposeSubjectGroup(scene, subjectGroupRef.current, loadedTexturesRef.current);
      subjectGroupRef.current = null;
      loadedTexturesRef.current = [];
    };
  }, [mode, meshData, trackData, accentHex]);

  const triCount =
    meshData?.triangleCount ??
    (meshData?.indices?.length
      ? Math.floor(meshData.indices.length / 3)
      : meshData?.positions?.length
      ? Math.floor(meshData.positions.length / 9)
      : null);

  const showStudioBackdrop = !webglError && (!sceneReady || (loading && !meshData));

  return (
    <div
      className={`relative w-full h-full min-h-[200px] rounded-2xl overflow-hidden border border-white/[0.08] bg-[#0B1019] select-none ${className}`}
    >
      {/* WebGL Canvas Mount */}
      <div
        ref={containerRef}
        onDoubleClick={resetCamera}
        className="w-full h-full cursor-grab active:cursor-grabbing"
      />

      {/* CSS Studio Backdrop before first frame or while mesh is loading */}
      {showStudioBackdrop && (
        <div
          className="absolute inset-0 flex items-center justify-center z-[5] pointer-events-none bg-[#0B1019]"
          style={{
            backgroundImage: `radial-gradient(ellipse at 50% 45%, ${accentHex}24 0%, rgba(11, 16, 25, 0.96) 68%), linear-gradient(to right, rgba(255,255,255,0.03) 1px, transparent 1px), linear-gradient(to bottom, rgba(255,255,255,0.03) 1px, transparent 1px)`,
            backgroundSize: "100% 100%, 36px 36px, 36px 36px",
          }}
        >
          <div className="px-4 py-2 rounded-xl bg-[#0D121B]/95 border border-white/[0.1] shadow-[0_12px_32px_rgba(0,0,0,0.65)] flex items-center gap-2.5 text-xs font-mono text-[var(--text-secondary)]">
            <span className="w-2 h-2 rounded-full bg-[var(--accent)] animate-pulse" />
            <span>Initializing 3D Showroom...</span>
          </div>
        </div>
      )}

      {/* WebGL Fallback Card */}
      {webglError && (
        <div className="absolute inset-0 flex items-center justify-center p-6 bg-[#0B1019]">
          <div className="max-w-sm rounded-2xl bg-white/[0.04] border border-white/[0.1] p-5 text-center space-y-2">
            <AlertTriangle className="w-6 h-6 text-amber-400 mx-auto" />
            <div className="text-sm font-semibold text-white">
              3D WebGL Viewport Unavailable
            </div>
            <p className="text-xs text-[var(--text-secondary)]">
              {webglError}
            </p>
          </div>
        </div>
      )}

      {/* Unified Non-Overlapping Top HUD Row */}
      <div className="absolute top-3 left-3 right-3 flex items-start justify-between gap-3 pointer-events-none z-10">
        {/* Top-left Label / Scene Telemetry Badge */}
        <div className="flex items-center gap-2 min-w-0 flex-1">
          <span className="px-2.5 py-1 rounded-lg bg-black/60 backdrop-blur-md border border-white/[0.09] text-[11px] font-mono text-[var(--text-secondary)] truncate">
            {label ?? (meshData ? "KN5 3D MESH" : mode === "track" ? "TRACK RIBBON" : "STUDIO PREVIEW")}
          </span>
          {triCount !== null && (
            <span className="px-2 py-1 rounded-lg bg-black/60 backdrop-blur-md border border-white/[0.08] text-[10px] font-mono text-[var(--text-muted)] tabular-nums shrink-0 whitespace-nowrap">
              {triCount.toLocaleString()} tris
            </span>
          )}
        </div>

        {/* Top-right Floating Toolbar Pills */}
        <div className="pointer-events-auto shrink-0 flex items-center gap-1 p-1 rounded-xl bg-black/60 backdrop-blur-md border border-white/[0.09]">
          <button
            type="button"
            onClick={() => setWireframe(false)}
            className={`h-7 px-2.5 rounded-lg text-[11px] font-medium flex items-center gap-1.5 transition-colors cursor-pointer whitespace-nowrap ${
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
            className={`h-7 px-2.5 rounded-lg text-[11px] font-medium flex items-center gap-1.5 transition-colors cursor-pointer whitespace-nowrap ${
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
            className={`h-7 px-2.5 rounded-lg text-[11px] font-medium flex items-center gap-1.5 transition-colors cursor-pointer whitespace-nowrap ${
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
      </div>

      {/* Bottom-left Control Hint */}
      <div className="absolute bottom-2.5 left-3 text-[10px] font-mono text-white/40 pointer-events-none">
        Drag to Orbit · Shift/Right-Drag to Pan · Scroll to Zoom
      </div>
    </div>
  );
}
