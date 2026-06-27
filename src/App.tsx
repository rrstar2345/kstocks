import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

type FetchStatus = "idle" | "loading" | "success" | "error";

interface StatusMessage {
  state: FetchStatus;
  message: string;
}

interface ExpiryResponse {
  expiry_dates: string[];
  strike_price: string[];
}

function App() {
  const [symbol, setSymb] = useState("NIFTY");
  const [expiryDates, setExpiryDates] = useState<string[]>([]);
  const [selectedExpiry, setSelectedExpiry] = useState("");
  const [expiryLoading, setExpiryLoading] = useState(false);
  const [status, setStatus] = useState<StatusMessage>({
    state: "idle",
    message: "",
  });

  // Fetch expiry dates when symbol changes
  useEffect(() => {
    const fetchExpiryDates = async () => {
      setExpiryLoading(true);
      try {
        const data = await invoke<ExpiryResponse>("fetch_expiry_dates", {
          symbol,
        });
        console.log("Received data from backend:", data);
        setExpiryDates(data.expiry_dates);
        // Set first expiry date by default
        if (data.expiry_dates.length > 0) {
          setSelectedExpiry(data.expiry_dates[0]);
          console.log("Auto-selected first expiry:", data.expiry_dates[0]);
        }
      } catch (error) {
        console.error("Failed to fetch expiry dates:", error);
        setExpiryDates([]);
        setSelectedExpiry("");
      } finally {
        setExpiryLoading(false);
      }
    };

    fetchExpiryDates();
  }, [symbol]);

  async function fetch_data() {
    setStatus({ state: "loading", message: "Starting data fetch..." });

    try {
      const result = await invoke<string>("store_ticks", {
        symbol,
        expiryDate: selectedExpiry,
      });
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
          disabled={status.state === "loading" || expiryLoading}
        >
          <option value="NIFTY">NIFTY 50</option>
          <option value="NIFTYNXT50">NIFTY Next 50</option>
          <option value="FINNIFTY">FIN NIFTY</option>
          <option value="BANKNIFTY">BANK NIFTY</option>
          <option value="MIDCPNIFTY">MIDCAP NIFTY</option>
        </select>

        <select
          id="expiry"
          value={selectedExpiry}
          onChange={(e) => setSelectedExpiry(e.target.value)}
          disabled={
            status.state === "loading" || expiryLoading || expiryDates.length === 0
          }
        >
          <option value="">
            {expiryLoading ? "Loading expiry dates..." : "Select Expiry Date"}
          </option>
          {expiryDates.map((date) => (
            <option key={date} value={date}>
              {date}
            </option>
          ))}
        </select>

        <button
          type="submit"
          disabled={
            status.state === "loading" || !selectedExpiry || expiryLoading
          }
        >
          {status.state === "loading" ? "Fetching..." : "Fetch"}
        </button>
      </form>

      <p>
        Selected Symbol: <strong>{symbol}</strong>
      </p>
      <p>
        Selected Expiry: <strong>{selectedExpiry || "Loading..."}</strong>
      </p>

      <div className={`status-box status-${status.state}`}>
        <p>Status: {status.message}</p>
      </div>
    </main>
  );
}

export default App;