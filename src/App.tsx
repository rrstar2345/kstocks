import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

type FetchStatus = "idle" | "loading" | "success" | "error";

interface StatusMessage {
  state: FetchStatus;
  message: string;
}

function App() {
  const [symbol, setSymb] = useState("NIFTY");
  const [status, setStatus] = useState<StatusMessage>({
    state: "idle",
    message: "",
  });

  async function fetch_data() {
    setStatus({ state: "loading", message: "Starting data fetch..." });

    try {
      const result = await invoke<string>("store_ticks", { symbol });
      setStatus({ state: "success", message: result });
    } catch (error) {
      const errorMsg =
        error instanceof Error ? error.message : String(error);
      setStatus({ state: "error", message: `Error: ${errorMsg}` });
      console.error("Fetch failed:", error);
    }
  }

  return (
    <main className="container">
      <h1>Welcome to KSTOCKS</h1>

      <form
        className="row"
        onSubmit={(e) => {
          e.preventDefault();
          fetch_data();
        }}
      >
        <select
          id="symb"
          value={symbol}
          onChange={(e) => setSymb(e.target.value)}
          disabled={status.state === "loading"}
        >
          <option value="NIFTY">NIFTY 50</option>
          <option value="NIFTYNXT50">NIFTY Next 50</option>
          <option value="FINNIFTY">FIN NIFTY</option>
          <option value="BANKNIFTY">BANK NIFTY</option>
          <option value="MIDCPNIFTY">MIDCAP NIFTY</option>
        </select>
        <button type="submit" disabled={status.state === "loading"}>
          {status.state === "loading" ? "Fetching..." : "Fetch"}
        </button>
      </form>

      <p>Selected Symbol: <strong>{symbol}</strong></p>
      
      <div className={`status-box status-${status.state}`}>
        <p>Status: {status.message}</p>
      </div>
    </main>
  );
}

export default App;
