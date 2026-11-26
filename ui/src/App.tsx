export function App() {
  return (
    <main className="flex min-h-screen flex-col items-center justify-center bg-neutral-950 px-6 text-neutral-100">
      <div className="flex items-center gap-3">
        <span className="inline-flex h-10 w-10 items-center justify-center rounded-lg bg-gradient-to-br from-orange-500 to-red-600 font-display text-2xl font-bold tracking-tight text-white shadow-lg shadow-orange-600/20">
          A
        </span>
        <div>
          <h1 className="font-display text-3xl font-bold uppercase tracking-wide text-white">
            ACXS Manager
          </h1>
          <p className="font-mono text-xs text-neutral-400">
            Tauri v2 + React 19 + Tailwind CSS v4
          </p>
        </div>
      </div>
    </main>
  );
}

export default App;
