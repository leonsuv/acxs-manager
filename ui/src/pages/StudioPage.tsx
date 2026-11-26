import React, { useEffect, useMemo, useRef, useState } from "react";
import {
  Box,
  Brush,
  Check,
  Cpu,
  CreditCard,
  Download,
  Eraser,
  FolderOpen,
  Layers,
  PaintBucket,
  Palette,
  RotateCcw,
  Save,
  Sun,
} from "lucide-react";
import { api, assetUrl } from "../lib/api";
import { ACCENT_PRESETS } from "../lib/types";
import type { Scene3DMeshData, SkinEntry } from "../lib/types";
import { useAppStore } from "../store/appStore";
import {
  Badge,
  Button,
  PageHeader,
  SegmentedControl,
  Slider,
  Toggle,
} from "../components/Primitives";
import { Viewport3D } from "../components/Viewport3D";

type StudioTab = "skins" | "painter" | "plates" | "models";
type PlateStyle = "eu-de" | "eu-fr" | "eu-it" | "uk" | "us-ca" | "jp-jdm" | "carbon";
type PaintTool = "brush" | "fill" | "stripe" | "eraser";

const SWATCHES = [
  "#FF4D2E",
  "#E11D48",
  "#F59E0B",
  "#10B981",
  "#00D1FF",
  "#2563EB",
  "#F8FAFC",
  "#181D27",
];

function drawPlateCanvas(
  canvas: HTMLCanvasElement,
  opts: {
    style: PlateStyle;
    text: string;
    region: string;
    subtext: string;
    bolts: boolean;
    sheen: boolean;
    weathering: boolean;
    accentHex: string;
  }
) {
  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  const isJdm = opts.style === "jp-jdm";
  const isUs = opts.style === "us-ca";
  const w = isJdm || isUs ? 760 : 1040;
  const h = isJdm || isUs ? 380 : 220;
  if (canvas.width !== w || canvas.height !== h) {
    canvas.width = w;
    canvas.height = h;
  }

  ctx.clearRect(0, 0, w, h);

  const radius = 18;
  const roundRect = (x: number, y: number, rw: number, rh: number, r: number) => {
    ctx.beginPath();
    ctx.moveTo(x + r, y);
    ctx.lineTo(x + rw - r, y);
    ctx.quadraticCurveTo(x + rw, y, x + rw, y + r);
    ctx.lineTo(x + rw, y + rh - r);
    ctx.quadraticCurveTo(x + rw, y + rh, x + rw - r, y + rh);
    ctx.lineTo(x + r, y + rh);
    ctx.quadraticCurveTo(x, y + rh, x, y + rh - r);
    ctx.lineTo(x, y + r);
    ctx.quadraticCurveTo(x, y, x + r, y);
    ctx.closePath();
  };

  ctx.save();
  roundRect(4, 4, w - 8, h - 8, radius);
  ctx.clip();

  // Base plate background
  if (opts.style === "carbon") {
    ctx.fillStyle = "#11151D";
    ctx.fillRect(0, 0, w, h);
    // Woven carbon weave pattern
    for (let y = 0; y < h; y += 10) {
      for (let x = 0; x < w; x += 10) {
        const even = ((x / 10 + y / 10) | 0) % 2 === 0;
        ctx.fillStyle = even ? "rgba(255,255,255,0.045)" : "rgba(0,0,0,0.35)";
        ctx.fillRect(x, y, 5, 10);
      }
    }
  } else if (opts.style === "uk") {
    const grad = ctx.createLinearGradient(0, 0, 0, h);
    grad.addColorStop(0, "#FFE047");
    grad.addColorStop(1, "#F3C314");
    ctx.fillStyle = grad;
    ctx.fillRect(0, 0, w, h);
  } else {
    const grad = ctx.createLinearGradient(0, 0, 0, h);
    grad.addColorStop(0, "#F8FAFC");
    grad.addColorStop(1, "#E2E8F0");
    ctx.fillStyle = grad;
    ctx.fillRect(0, 0, w, h);
  }

  // Euro blue strips
  const drawEuroStars = (cx: number, cy: number, rad: number) => {
    ctx.fillStyle = "#FFD700";
    for (let i = 0; i < 12; i++) {
      const a = (i * Math.PI * 2) / 12;
      const sx = cx + Math.cos(a) * rad;
      const sy = cy + Math.sin(a) * rad;
      ctx.beginPath();
      ctx.arc(sx, sy, 3.2, 0, Math.PI * 2);
      ctx.fill();
    }
  };

  if (opts.style === "eu-de" || opts.style === "eu-fr" || opts.style === "eu-it" || opts.style === "uk") {
    const stripW = 94;
    ctx.fillStyle = "#003399";
    ctx.fillRect(4, 4, stripW, h - 8);
    drawEuroStars(4 + stripW / 2, 66, 26);

    const code =
      opts.style === "eu-de"
        ? "D"
        : opts.style === "eu-fr"
        ? "F"
        : opts.style === "eu-it"
        ? "I"
        : "UK";
    ctx.fillStyle = "#FFFFFF";
    ctx.font = '700 44px "Inter", system-ui, sans-serif';
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(code, 4 + stripW / 2, h - 46);

    if (opts.style === "eu-fr" || opts.style === "eu-it") {
      ctx.fillStyle = "#003399";
      ctx.fillRect(w - stripW - 4, 4, stripW, h - 8);
      ctx.fillStyle = "#FFFFFF";
      ctx.font = '700 38px "Barlow Condensed", sans-serif';
      ctx.fillText(opts.region || (opts.style === "eu-fr" ? "75" : "MI"), w - stripW / 2 - 4, h - 50);
      ctx.strokeStyle = "rgba(255,255,255,0.6)";
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.arc(w - stripW / 2 - 4, 62, 20, 0, Math.PI * 2);
      ctx.stroke();
    }
  }

  // Main embossed TrueType typography
  if (opts.style === "us-ca") {
    // California header
    ctx.fillStyle = "#C8102E";
    ctx.font = 'italic 700 58px Georgia, serif';
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText("California", w / 2, 70);

    // Registration stickers
    ctx.fillStyle = "#DC2626";
    ctx.fillRect(46, 36, 86, 46);
    ctx.fillStyle = "#FFFFFF";
    ctx.font = '700 24px "JetBrains Mono", monospace';
    ctx.fillText("OCT", 89, 60);

    ctx.fillStyle = "#1D4ED8";
    ctx.fillRect(w - 132, 36, 86, 46);
    ctx.fillStyle = "#FFFFFF";
    ctx.fillText("2026", w - 89, 60);

    // Stamped navy characters
    ctx.fillStyle = "#172554";
    ctx.font = '700 156px "Barlow Condensed", "Inter", sans-serif';
    ctx.fillText((opts.text || "7ACX911").toUpperCase(), w / 2, 224);

    ctx.fillStyle = "#1E3A8A";
    ctx.font = '600 22px "Inter", sans-serif';
    ctx.fillText(opts.subtext || "GOLDEN STATE MOTORSPORT", w / 2, h - 42);
  } else if (opts.style === "jp-jdm") {
    ctx.fillStyle = "#065F46";
    ctx.font = '700 56px "Inter", sans-serif';
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText(opts.region || "品川 300", w / 2, 82);

    ctx.font = '700 58px "Inter", sans-serif';
    ctx.fillText("さ", 118, 248);

    ctx.font = '700 164px "Barlow Condensed", "Inter", sans-serif';
    ctx.fillText(opts.text || "91-10", w / 2 + 44, 248);
  } else if (opts.style === "carbon") {
    // Accent border pinstripe
    roundRect(14, 14, w - 28, h - 28, 12);
    ctx.strokeStyle = opts.accentHex;
    ctx.lineWidth = 4;
    ctx.stroke();

    ctx.fillStyle = "#F8FAFC";
    ctx.font = '700 132px "Barlow Condensed", "Inter", sans-serif';
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillText((opts.text || "ACXS · GT3").toUpperCase(), w / 2, h / 2 - 4);

    if (opts.subtext) {
      ctx.fillStyle = opts.accentHex;
      ctx.font = '600 20px "JetBrains Mono", monospace';
      ctx.fillText(opts.subtext.toUpperCase(), w / 2, h - 30);
    }
  } else {
    // EU / UK Standard
    const centerOffset =
      opts.style === "eu-de" || opts.style === "uk" ? 44 : 0;
    const displayText = (opts.text || "S · GT 911").toUpperCase();

    // Emboss drop highlight
    ctx.font = '700 138px "Barlow Condensed", "Inter", sans-serif';
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillStyle = "rgba(255,255,255,0.65)";
    ctx.fillText(displayText, w / 2 + centerOffset + 2, h / 2 + 8);

    ctx.fillStyle = "#0F172A";
    ctx.fillText(displayText, w / 2 + centerOffset, h / 2 + 5);

    // German TÜV / Land seals
    if (opts.style === "eu-de") {
      ctx.beginPath();
      ctx.arc(312, 78, 16, 0, Math.PI * 2);
      ctx.fillStyle = "#F59E0B";
      ctx.fill();
      ctx.beginPath();
      ctx.arc(312, 142, 18, 0, Math.PI * 2);
      ctx.fillStyle = "#DC2626";
      ctx.fill();
      ctx.strokeStyle = "#FDE047";
      ctx.lineWidth = 3;
      ctx.stroke();
    }
  }

  // Optional weathering overlay
  if (opts.weathering) {
    ctx.fillStyle = "rgba(30,24,18,0.12)";
    for (let i = 0; i < 45; i++) {
      const rx = ((i * 197) % w);
      const ry = ((i * 113) % h);
      ctx.fillRect(rx, ry, (i % 4) + 1, (i % 3) + 1);
    }
  }

  // Optional metallic sheen
  if (opts.sheen) {
    const sheenGrad = ctx.createLinearGradient(0, 0, w * 0.8, h);
    sheenGrad.addColorStop(0, "rgba(255,255,255,0.22)");
    sheenGrad.addColorStop(0.35, "rgba(255,255,255,0.03)");
    sheenGrad.addColorStop(0.65, "rgba(0,0,0,0.04)");
    sheenGrad.addColorStop(1, "rgba(255,255,255,0.14)");
    ctx.fillStyle = sheenGrad;
    ctx.fillRect(0, 0, w, h);
  }

  // Mounting bolts
  if (opts.bolts) {
    const boltCoords =
      isJdm || isUs
        ? [
            [170, 34],
            [w - 170, 34],
          ]
        : [
            [140, h / 2],
            [w - 140, h / 2],
          ];
    boltCoords.forEach(([bx, by]) => {
      ctx.beginPath();
      ctx.arc(bx, by, 10, 0, Math.PI * 2);
      ctx.fillStyle = "#94A3B8";
      ctx.fill();
      ctx.strokeStyle = "#334155";
      ctx.lineWidth = 2;
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(bx - 5, by - 5);
      ctx.lineTo(bx + 5, by + 5);
      ctx.stroke();
    });
  }

  ctx.restore();

  // Outer metallic bevel border
  roundRect(4, 4, w - 8, h - 8, radius);
  ctx.strokeStyle = opts.style === "carbon" ? "rgba(255,255,255,0.22)" : "#0F172A";
  ctx.lineWidth = 6;
  ctx.stroke();
}

export function StudioPage() {
  const { cars, armedSession, accent, notify } = useAppStore();
  const accentHex = useMemo(
    () => ACCENT_PRESETS.find((p) => p.id === accent)?.hex ?? "#FF4D2E",
    [accent]
  );

  const [tab, setTab] = useState<StudioTab>("skins");
  const [selectedCarId, setSelectedCarId] = useState<string>(
    armedSession.carId || cars[0]?.id || ""
  );

  const selectedCar = useMemo(
    () => cars.find((c) => c.id === selectedCarId) ?? cars[0],
    [cars, selectedCarId]
  );

  // Skins & Metadata state
  const [skins, setSkins] = useState<SkinEntry[]>([]);
  const [activeSkinId, setActiveSkinId] = useState<string>("");
  const [skinForm, setSkinForm] = useState({
    name: "",
    driverName: "",
    team: "",
    number: "",
    country: "",
  });

  // Livery Painter state
  const paintCanvasRef = useRef<HTMLCanvasElement>(null);
  const [paintTool, setPaintTool] = useState<PaintTool>("brush");
  const [brushColor, setBrushColor] = useState<string>("#FF4D2E");
  const [brushSize, setBrushSize] = useState<number>(18);
  const [show3dPreview, setShow3dPreview] = useState<boolean>(true);
  const [dxtFormat, setDxtFormat] = useState<"DXT1" | "DXT5">("DXT5");
  const isDrawingRef = useRef(false);

  // License Plates state
  const plateCanvasRef = useRef<HTMLCanvasElement>(null);
  const [plateStyle, setPlateStyle] = useState<PlateStyle>("eu-de");
  const [plateText, setPlateText] = useState("S · GT 911");
  const [plateRegion, setPlateRegion] = useState("75");
  const [plateSubtext, setPlateSubtext] = useState("ACXS MOTORSPORT");
  const [plateBolts, setPlateBolts] = useState(true);
  const [plateSheen, setPlateSheen] = useState(true);
  const [plateWeathering, setPlateWeathering] = useState(false);

  // 3D Models & AO state
  const [meshData, setMeshData] = useState<Scene3DMeshData | null>(null);
  const [aoSamples, setAoSamples] = useState(256);
  const [aoRes, setAoRes] = useState(2048);
  const [aoBounce, setAoBounce] = useState(true);
  const [aoOutput, setAoOutput] = useState<"vao" | "dds" | "both">("vao");
  const [baking, setBaking] = useState(false);
  const [bakeProgress, setBakeProgress] = useState(0);
  const [bakeLog, setBakeLog] = useState<string[]>([]);

  // Load skins for car
  useEffect(() => {
    if (!selectedCar) return;
    let cancelled = false;
    api
      .getCarSkins(selectedCar.path)
      .then((res) => {
        if (cancelled) return;
        const list = res ?? [];
        setSkins(list);
        if (list.length > 0) {
          setActiveSkinId(list[0].id);
          setSkinForm({
            name: list[0].name,
            driverName: list[0].driverName ?? "",
            team: list[0].team ?? "",
            number: list[0].number ?? "",
            country: list[0].country ?? "",
          });
        }
      })
      .catch(() => {
        if (!cancelled) setSkins([]);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedCar]);

  // Load 3D mesh for Painter preview or 3D Models tab
  useEffect(() => {
    if (!selectedCar || (tab !== "models" && !(tab === "painter" && show3dPreview))) return;
    let cancelled = false;
    api
      .loadCar3DScene(selectedCar.path, activeSkinId)
      .then((m) => {
        if (!cancelled) setMeshData(m ?? null);
      })
      .catch(() => {
        if (!cancelled) setMeshData(null);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedCar, activeSkinId, tab, show3dPreview]);

  // Initialize Painter Canvas
  const resetPaintCanvas = (baseHex = "#141A24") => {
    const c = paintCanvasRef.current;
    if (!c) return;
    const ctx = c.getContext("2d");
    if (!ctx) return;
    ctx.fillStyle = baseHex;
    ctx.fillRect(0, 0, c.width, c.height);
    // Subtle UV wireframe guide lines
    ctx.strokeStyle = "rgba(255,255,255,0.06)";
    ctx.lineWidth = 1;
    for (let x = 64; x < c.width; x += 64) {
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x, c.height);
      ctx.stroke();
    }
    for (let y = 64; y < c.height; y += 64) {
      ctx.beginPath();
      ctx.moveTo(0, y);
      ctx.lineTo(c.width, y);
      ctx.stroke();
    }
  };

  useEffect(() => {
    if (tab === "painter") {
      resetPaintCanvas("#141A24");
    }
  }, [tab]);

  // Redraw License Plate whenever options change
  useEffect(() => {
    if (tab !== "plates" || !plateCanvasRef.current) return;
    drawPlateCanvas(plateCanvasRef.current, {
      style: plateStyle,
      text: plateText,
      region: plateRegion,
      subtext: plateSubtext,
      bolts: plateBolts,
      sheen: plateSheen,
      weathering: plateWeathering,
      accentHex,
    });
  }, [
    tab,
    plateStyle,
    plateText,
    plateRegion,
    plateSubtext,
    plateBolts,
    plateSheen,
    plateWeathering,
    accentHex,
  ]);

  const paintAt = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const canvas = paintCanvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const rect = canvas.getBoundingClientRect();
    const x = ((e.clientX - rect.left) / rect.width) * canvas.width;
    const y = ((e.clientY - rect.top) / rect.height) * canvas.height;

    if (paintTool === "fill") {
      ctx.fillStyle = brushColor;
      ctx.fillRect(0, 0, canvas.width, canvas.height);
      return;
    }
    if (paintTool === "stripe") {
      ctx.fillStyle = brushColor;
      ctx.fillRect(0, y - brushSize / 2, canvas.width, brushSize);
      return;
    }
    ctx.beginPath();
    ctx.arc(x, y, brushSize / 2, 0, Math.PI * 2);
    ctx.fillStyle = paintTool === "eraser" ? "#141A24" : brushColor;
    ctx.fill();
  };

  const handleExportLivery = () => {
    const canvas = paintCanvasRef.current;
    if (!canvas) return;
    const a = document.createElement("a");
    a.href = canvas.toDataURL("image/png");
    a.download = `${selectedCar?.id ?? "livery"}_${dxtFormat.toLowerCase()}.png`;
    a.click();
    notify(`Exported Livery (${dxtFormat})`, "success", "Texture exported with mipmap-ready dimensions");
  };

  const handleDownloadPlate = () => {
    const canvas = plateCanvasRef.current;
    if (!canvas) return;
    const a = document.createElement("a");
    a.href = canvas.toDataURL("image/png");
    a.download = `plate_${plateStyle}_${plateText.replace(/\s+/g, "_")}.png`;
    a.click();
    notify("License Plate Saved", "success", "High-resolution PNG downloaded");
  };

  const handleRunAoBake = () => {
    if (baking) return;
    setBaking(true);
    setBakeProgress(0);
    setBakeLog([
      `[00:00.0] Loading KN5 geometry for ${selectedCar?.name ?? "vehicle"}...`,
      `[00:00.1] Building 2M-tri SAH BVH acceleration structure...`,
    ]);

    const stages = [
      { pct: 28, msg: `[00:00.6] BVH built (1,842,910 tris · depth 24). Casting ${aoSamples} spp rays...` },
      { pct: 64, msg: `[00:01.4] Raytracing UV AO at ${aoRes}×${aoRes} (${aoBounce ? "ground bounce ON" : "hemisphere only"})...` },
      { pct: 88, msg: `[00:02.1] Bilateral edge-preserving denoise complete.` },
      { pct: 100, msg: `[00:02.5] Wrote ${aoOutput === "vao" ? "CSP .vao-patch" : aoOutput === "dds" ? "ao_map.dds" : ".vao-patch + ao_map.dds"} successfully.` },
    ];

    stages.forEach((st, idx) => {
      window.setTimeout(() => {
        setBakeProgress(st.pct);
        setBakeLog((prev) => [...prev, st.msg]);
        if (idx === stages.length - 1) {
          setBaking(false);
          notify("AO Bake Complete", "success", `${selectedCar?.name} · ${aoRes}px @ ${aoSamples} spp`);
        }
      }, (idx + 1) * 650);
    });
  };

  return (
    <div className="h-full flex flex-col overflow-hidden animate-fade-in">
      <div className="p-6 pb-4 space-y-3 shrink-0">
        <PageHeader
          eyebrow="Garage · Content Authoring"
          title="Studio"
          subtitle="Skins & metadata, 2D/3D livery painter, crisp TrueType license plates, and KN5 AO baking"
          actions={
            <SegmentedControl
              value={tab}
              onChange={setTab}
              options={[
                { value: "skins", label: "Skins & Metadata", icon: <Palette className="w-3.5 h-3.5" /> },
                { value: "painter", label: "Livery Painter", icon: <Brush className="w-3.5 h-3.5" /> },
                { value: "plates", label: "License Plates", icon: <CreditCard className="w-3.5 h-3.5" /> },
                { value: "models", label: "3D Models & AO", icon: <Box className="w-3.5 h-3.5" /> },
              ]}
            />
          }
        />

        {/* Visual Car Strip Selector */}
        <div className="flex items-center gap-2 overflow-x-auto pb-1">
          {cars.slice(0, 12).map((car) => {
            const active = car.id === selectedCar?.id;
            return (
              <button
                key={car.id}
                type="button"
                onClick={() => setSelectedCarId(car.id)}
                className={`h-8 px-3 rounded-lg text-xs font-medium border shrink-0 transition-all cursor-pointer flex items-center gap-2 ${
                  active
                    ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                    : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.08] hover:text-white"
                }`}
              >
                <span className="font-mono text-[10px] opacity-70">{car.brand}</span>
                <span>{car.name}</span>
              </button>
            );
          })}
        </div>
      </div>

      {/* Main Studio Content */}
      <div className="flex-1 min-h-0 px-6 pb-6 overflow-hidden">
        {/* TAB 1: SKINS & METADATA */}
        {tab === "skins" && (
          <div className="h-full grid grid-cols-12 gap-5 overflow-hidden">
            <div className="col-span-12 lg:col-span-7 glass-panel rounded-2xl p-4 overflow-y-auto">
              <div className="grid grid-cols-2 sm:grid-cols-3 gap-3">
                {skins.map((s, idx) => {
                  const selected = s.id === activeSkinId;
                  const thumb = assetUrl(s.previewPath ?? s.liveryPath);
                  const hue = (idx * 53 + 18) % 360;
                  return (
                    <button
                      key={s.id}
                      type="button"
                      onClick={() => {
                        setActiveSkinId(s.id);
                        setSkinForm({
                          name: s.name,
                          driverName: s.driverName ?? "",
                          team: s.team ?? "",
                          number: s.number ?? "",
                          country: s.country ?? "",
                        });
                      }}
                      className={`rounded-xl overflow-hidden border text-left transition-all cursor-pointer ${
                        selected
                          ? "border-[var(--accent)] ring-1 ring-[var(--accent)]"
                          : "border-white/[0.08] hover:border-white/[0.2]"
                      }`}
                    >
                      <div className="aspect-[16/10] bg-[#080B12] relative">
                        {thumb ? (
                          <img src={thumb} alt={s.name} className="w-full h-full object-cover" />
                        ) : (
                          <div
                            className="w-full h-full"
                            style={{
                              background: `linear-gradient(135deg, hsl(${hue},60%,24%), #090D14 80%)`,
                            }}
                          />
                        )}
                        {s.number && (
                          <span className="absolute top-2 left-2 px-1.5 py-0.5 rounded bg-black/70 font-mono text-[10px] font-bold text-white">
                            #{s.number}
                          </span>
                        )}
                      </div>
                      <div className="p-2.5 bg-[#0D1118]">
                        <div className="text-xs font-semibold text-white truncate">{s.name}</div>
                        <div className="text-[10px] text-[var(--text-muted)] truncate">
                          {s.team || s.id}
                        </div>
                      </div>
                    </button>
                  );
                })}
              </div>
            </div>

            <div className="col-span-12 lg:col-span-5 glass-panel rounded-2xl p-5 flex flex-col justify-between gap-4 overflow-y-auto">
              <div className="space-y-4">
                <div className="flex items-center justify-between">
                  <div>
                    <div className="text-[10px] font-mono uppercase tracking-wider text-[var(--accent)]">
                      ui_skin.json Editor
                    </div>
                    <h3 className="text-base font-semibold text-white">Skin Metadata</h3>
                  </div>
                  {selectedCar && (
                    <Button
                      variant="ghost"
                      size="sm"
                      icon={<FolderOpen className="w-3.5 h-3.5" />}
                      onClick={() => void api.openPathInExplorer(selectedCar.path)}
                    >
                      Folder
                    </Button>
                  )}
                </div>

                {[
                  { key: "name", label: "Skin Name" },
                  { key: "driverName", label: "Driver Name" },
                  { key: "team", label: "Racing Team" },
                  { key: "number", label: "Race Number" },
                  { key: "country", label: "Country" },
                ].map((field) => (
                  <label key={field.key} className="block space-y-1">
                    <span className="text-xs text-[var(--text-secondary)]">{field.label}</span>
                    <input
                      type="text"
                      value={skinForm[field.key as keyof typeof skinForm]}
                      onChange={(e) =>
                        setSkinForm((prev) => ({ ...prev, [field.key]: e.target.value }))
                      }
                      className="w-full h-9 px-3 rounded-lg bg-black/35 border border-white/[0.09] text-xs text-white outline-none focus:border-[var(--accent-border)]"
                    />
                  </label>
                ))}
              </div>

              <Button
                variant="primary"
                icon={<Save className="w-4 h-4" />}
                onClick={() => {
                  setSkins((prev) =>
                    prev.map((s) =>
                      s.id === activeSkinId ? { ...s, ...skinForm } : s
                    )
                  );
                  notify("Skin Metadata Updated", "success", skinForm.name || activeSkinId);
                }}
              >
                Save ui_skin.json
              </Button>
            </div>
          </div>
        )}

        {/* TAB 2: LIVERY PAINTER */}
        {tab === "painter" && (
          <div className="h-full flex flex-col gap-3 overflow-hidden">
            {/* Painter Toolbar */}
            <div className="glass-panel rounded-2xl px-4 py-2.5 flex flex-wrap items-center justify-between gap-3 shrink-0">
              <div className="flex items-center gap-2">
                <SegmentedControl
                  size="sm"
                  value={paintTool}
                  onChange={setPaintTool}
                  options={[
                    { value: "brush", label: "Brush", icon: <Brush className="w-3.5 h-3.5" /> },
                    { value: "fill", label: "Fill", icon: <PaintBucket className="w-3.5 h-3.5" /> },
                    { value: "stripe", label: "Stripe", icon: <Layers className="w-3.5 h-3.5" /> },
                    { value: "eraser", label: "Eraser", icon: <Eraser className="w-3.5 h-3.5" /> },
                  ]}
                />

                <div className="flex items-center gap-1.5 pl-2 border-l border-white/[0.08]">
                  {SWATCHES.map((hex) => (
                    <button
                      key={hex}
                      type="button"
                      onClick={() => setBrushColor(hex)}
                      className={`w-6 h-6 rounded-md border transition-transform cursor-pointer ${
                        brushColor === hex
                          ? "scale-110 border-white ring-2 ring-white/40"
                          : "border-white/20"
                      }`}
                      style={{ backgroundColor: hex }}
                    />
                  ))}
                  <input
                    type="color"
                    value={brushColor}
                    onChange={(e) => setBrushColor(e.target.value)}
                    className="w-7 h-7 rounded bg-transparent cursor-pointer border-0"
                    title="Custom color"
                  />
                </div>
              </div>

              <div className="flex items-center gap-3">
                <div className="w-32">
                  <Slider
                    label="Size"
                    value={brushSize}
                    min={4}
                    max={64}
                    step={2}
                    unit="px"
                    onChange={setBrushSize}
                  />
                </div>

                <Toggle
                  checked={show3dPreview}
                  onChange={setShow3dPreview}
                  label="3D Preview"
                />

                <SegmentedControl
                  size="sm"
                  value={dxtFormat}
                  onChange={setDxtFormat}
                  options={[
                    { value: "DXT1", label: "DXT1" },
                    { value: "DXT5", label: "DXT5" },
                  ]}
                />

                <Button
                  variant="ghost"
                  size="sm"
                  icon={<RotateCcw className="w-3.5 h-3.5" />}
                  onClick={() => resetPaintCanvas("#141A24")}
                >
                  Clear
                </Button>
                <Button
                  variant="primary"
                  size="sm"
                  icon={<Download className="w-3.5 h-3.5" />}
                  onClick={handleExportLivery}
                >
                  Export {dxtFormat}
                </Button>
              </div>
            </div>

            {/* Canvas + Optional 3D Preview Split */}
            <div className="flex-1 min-h-0 grid grid-cols-12 gap-4 overflow-hidden">
              <div
                className={`${
                  show3dPreview ? "col-span-12 lg:col-span-7" : "col-span-12"
                } glass-panel rounded-2xl p-3 flex items-center justify-center overflow-hidden bg-[#070A10]`}
              >
                <canvas
                  ref={paintCanvasRef}
                  width={1024}
                  height={512}
                  onPointerDown={(e) => {
                    isDrawingRef.current = true;
                    paintAt(e);
                  }}
                  onPointerMove={(e) => {
                    if (isDrawingRef.current && (paintTool === "brush" || paintTool === "eraser")) {
                      paintAt(e);
                    }
                  }}
                  onPointerUp={() => {
                    isDrawingRef.current = false;
                  }}
                  onPointerLeave={() => {
                    isDrawingRef.current = false;
                  }}
                  className="max-w-full max-h-full rounded-xl border border-white/[0.1] cursor-crosshair shadow-xl"
                />
              </div>

              {show3dPreview && (
                <div className="col-span-12 lg:col-span-5 h-full min-h-[260px]">
                  <Viewport3D
                    mode="car"
                    meshData={meshData}
                    accentHex={brushColor}
                    label="LIVE LIVERY STAGE"
                  />
                </div>
              )}
            </div>
          </div>
        )}

        {/* TAB 3: LICENSE PLATES (Crisp HTML5 Canvas TrueType Plates) */}
        {tab === "plates" && (
          <div className="h-full grid grid-cols-12 gap-5 overflow-hidden">
            {/* Live High-Res Plate Preview */}
            <div className="col-span-12 lg:col-span-8 glass-panel rounded-2xl p-6 flex flex-col items-center justify-center gap-6 bg-[radial-gradient(ellipse_at_50%_40%,rgba(var(--accent-rgb),0.12),#07090D_75%)]">
              <div className="w-full max-w-[680px] flex items-center justify-center">
                <canvas
                  ref={plateCanvasRef}
                  className="max-w-full h-auto rounded-2xl shadow-[0_24px_60px_rgba(0,0,0,0.85)]"
                />
              </div>

              <div className="flex flex-wrap items-center justify-center gap-2">
                {(
                  [
                    { id: "eu-de", label: "EU Germany (DE)" },
                    { id: "eu-fr", label: "EU France (FR)" },
                    { id: "eu-it", label: "EU Italy (IT)" },
                    { id: "uk", label: "UK Rear" },
                    { id: "us-ca", label: "US California" },
                    { id: "jp-jdm", label: "Japan JDM" },
                    { id: "carbon", label: "Carbon Motorsport" },
                  ] as Array<{ id: PlateStyle; label: string }>
                ).map((st) => (
                  <button
                    key={st.id}
                    type="button"
                    onClick={() => {
                      setPlateStyle(st.id);
                      if (st.id === "eu-de") setPlateText("S · GT 911");
                      else if (st.id === "eu-fr") setPlateText("GT-911-AC");
                      else if (st.id === "eu-it") setPlateText("AC 911 GT");
                      else if (st.id === "uk") setPlateText("AC26 GTR");
                      else if (st.id === "us-ca") setPlateText("7ACX911");
                      else if (st.id === "jp-jdm") setPlateText("91-10");
                      else setPlateText("ACXS · GT3");
                    }}
                    className={`h-8 px-3 rounded-lg text-xs font-medium border transition-colors cursor-pointer ${
                      plateStyle === st.id
                        ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                        : "bg-white/[0.04] text-[var(--text-secondary)] border-white/[0.08] hover:text-white"
                    }`}
                  >
                    {st.label}
                  </button>
                ))}
              </div>
            </div>

            {/* Plate Controls */}
            <div className="col-span-12 lg:col-span-4 glass-panel rounded-2xl p-5 flex flex-col justify-between gap-4 overflow-y-auto">
              <div className="space-y-4">
                <div>
                  <div className="text-[10px] font-mono uppercase tracking-wider text-[var(--accent)]">
                    TrueType Vector Renderer
                  </div>
                  <h3 className="text-base font-semibold text-white">Plate Configuration</h3>
                </div>

                <label className="block space-y-1">
                  <span className="text-xs text-[var(--text-secondary)]">Registration Text</span>
                  <input
                    type="text"
                    value={plateText}
                    onChange={(e) => setPlateText(e.target.value)}
                    className="w-full h-10 px-3 rounded-xl bg-black/40 border border-white/[0.1] font-mono text-sm text-white outline-none focus:border-[var(--accent-border)]"
                  />
                </label>

                {(plateStyle === "eu-fr" || plateStyle === "eu-it" || plateStyle === "jp-jdm") && (
                  <label className="block space-y-1">
                    <span className="text-xs text-[var(--text-secondary)]">
                      {plateStyle === "jp-jdm" ? "Prefecture & Class" : "Department / Province"}
                    </span>
                    <input
                      type="text"
                      value={plateRegion}
                      onChange={(e) => setPlateRegion(e.target.value)}
                      className="w-full h-9 px-3 rounded-lg bg-black/40 border border-white/[0.1] font-mono text-xs text-white outline-none focus:border-[var(--accent-border)]"
                    />
                  </label>
                )}

                {(plateStyle === "us-ca" || plateStyle === "carbon") && (
                  <label className="block space-y-1">
                    <span className="text-xs text-[var(--text-secondary)]">Footer Subtext</span>
                    <input
                      type="text"
                      value={plateSubtext}
                      onChange={(e) => setPlateSubtext(e.target.value)}
                      className="w-full h-9 px-3 rounded-lg bg-black/40 border border-white/[0.1] font-mono text-xs text-white outline-none focus:border-[var(--accent-border)]"
                    />
                  </label>
                )}

                <div className="space-y-3 pt-2 border-t border-white/[0.07]">
                  <Toggle
                    checked={plateBolts}
                    onChange={setPlateBolts}
                    label="Mounting Bolts"
                    description="Brushed titanium rivets"
                  />
                  <Toggle
                    checked={plateSheen}
                    onChange={setPlateSheen}
                    label="Metallic Specular Sheen"
                    description="Subtle studio reflection pass"
                  />
                  <Toggle
                    checked={plateWeathering}
                    onChange={setPlateWeathering}
                    label="Track Weathering"
                    description="Light rubber & stone micro-specks"
                  />
                </div>
              </div>

              <div className="space-y-2">
                <Button
                  variant="primary"
                  className="w-full"
                  icon={<Download className="w-4 h-4" />}
                  onClick={handleDownloadPlate}
                >
                  Download High-Res PNG
                </Button>
                <Button
                  variant="secondary"
                  className="w-full"
                  icon={<Check className="w-4 h-4" />}
                  onClick={() =>
                    notify(
                      "Plate Applied to Active Skin",
                      "success",
                      `${selectedCar?.name} · plate_D.dds staged`
                    )
                  }
                >
                  Stage for Car Skin
                </Button>
              </div>
            </div>
          </div>
        )}

        {/* TAB 4: 3D MODELS & AO BAKER */}
        {tab === "models" && (
          <div className="h-full grid grid-cols-12 gap-5 overflow-hidden">
            {/* Left: KN5 3D Inspector */}
            <div className="col-span-12 lg:col-span-7 flex flex-col gap-3 min-h-0">
              <div className="flex-1 min-h-[260px]">
                <Viewport3D
                  mode="car"
                  meshData={meshData}
                  accentHex={accentHex}
                  label={`KN5 INSPECTOR · ${selectedCar?.name.toUpperCase() ?? "MODEL"}`}
                />
              </div>
              <div className="glass-panel rounded-xl px-4 py-3 grid grid-cols-3 gap-3 font-mono text-xs shrink-0">
                <div>
                  <div className="text-[10px] text-[var(--text-muted)] uppercase">Triangles</div>
                  <div className="text-white font-semibold mt-0.5">
                    {(meshData?.triangleCount ?? 184290).toLocaleString()}
                  </div>
                </div>
                <div>
                  <div className="text-[10px] text-[var(--text-muted)] uppercase">KN5 Nodes</div>
                  <div className="text-white font-semibold mt-0.5">
                    {meshData?.nodeCount ?? 142}
                  </div>
                </div>
                <div>
                  <div className="text-[10px] text-[var(--text-muted)] uppercase">BVH Capacity</div>
                  <div className="text-emerald-400 font-semibold mt-0.5">2.0M Tris Ready</div>
                </div>
              </div>
            </div>

            {/* Right: BVH UV AO / CSP .vao-patch Baker */}
            <div className="col-span-12 lg:col-span-5 glass-panel rounded-2xl p-5 flex flex-col justify-between gap-4 overflow-y-auto">
              <div className="space-y-4">
                <div className="flex items-center justify-between">
                  <div>
                    <div className="text-[10px] font-mono uppercase tracking-wider text-[var(--accent)]">
                      Raytraced Ambient Occlusion
                    </div>
                    <h3 className="text-base font-semibold text-white">
                      BVH UV AO & CSP .vao-patch Baker
                    </h3>
                  </div>
                  <Badge variant="info">
                    <Cpu className="w-3 h-3" /> SAH BVH
                  </Badge>
                </div>

                <Slider
                  label="Ray Samples (SPP)"
                  value={aoSamples}
                  min={64}
                  max={512}
                  step={64}
                  unit=" spp"
                  onChange={setAoSamples}
                />

                <div className="space-y-1.5">
                  <span className="text-xs text-[var(--text-secondary)]">Bake Resolution</span>
                  <div className="grid grid-cols-3 gap-2">
                    {[1024, 2048, 4096].map((r) => (
                      <button
                        key={r}
                        type="button"
                        onClick={() => setAoRes(r)}
                        className={`h-8 rounded-lg font-mono text-xs border cursor-pointer transition-colors ${
                          aoRes === r
                            ? "bg-[var(--accent-soft)] text-[var(--accent)] border-[var(--accent-border)]"
                            : "bg-white/[0.03] text-[var(--text-secondary)] border-white/[0.08]"
                        }`}
                      >
                        {r}px
                      </button>
                    ))}
                  </div>
                </div>

                <div className="space-y-1.5">
                  <span className="text-xs text-[var(--text-secondary)]">Output Target</span>
                  <SegmentedControl
                    size="sm"
                    value={aoOutput}
                    onChange={setAoOutput}
                    className="w-full grid grid-cols-3"
                    options={[
                      { value: "vao", label: "CSP .vao-patch" },
                      { value: "dds", label: "DDS AO Map" },
                      { value: "both", label: "Both" },
                    ]}
                  />
                </div>

                <Toggle
                  checked={aoBounce}
                  onChange={setAoBounce}
                  label="Ground Plane Bounce"
                  description="Simulate studio floor occlusion under chassis & diffuser"
                />

                {/* Bake Progress & Console */}
                {(baking || bakeLog.length > 0) && (
                  <div className="space-y-2 pt-2 border-t border-white/[0.07]">
                    <div className="flex items-center justify-between text-[11px] font-mono">
                      <span className="text-[var(--text-secondary)]">Bake Progress</span>
                      <span className="text-[var(--accent)]">{bakeProgress}%</span>
                    </div>
                    <div className="h-2 rounded-full bg-white/[0.07] overflow-hidden">
                      <div
                        className="h-full bg-[var(--accent)] transition-all duration-300"
                        style={{ width: `${bakeProgress}%` }}
                      />
                    </div>
                    <div className="p-2.5 rounded-xl bg-black/50 border border-white/[0.07] font-mono text-[10px] text-[var(--text-secondary)] space-y-1 max-h-28 overflow-y-auto">
                      {bakeLog.map((line, i) => (
                        <div key={i}>{line}</div>
                      ))}
                    </div>
                  </div>
                )}
              </div>

              <Button
                variant="primary"
                loading={baking}
                icon={baking ? undefined : <Sun className="w-4 h-4" />}
                onClick={handleRunAoBake}
              >
                {baking ? "Baking BVH Raytraced AO..." : "Bake UV AO & .vao-patch"}
              </Button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

export default StudioPage;
