import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

type FetchStatus = "idle" | "loading" | "success" | "error";
type Page = "landing" | "detail";

interface StatusMessage {
  state: FetchStatus;
  message: string;
}

interface ExpiryResponse {
  expiry_dates: string[];
  strike_price: string[];
}

interface IndexCard {
  index_name: string;
  last_price: number;
  change: number;
  change_percent: number;
  is_positive: boolean;
  dissemination_time: string;
}

// interface SymbolInfo {
//   fno_index_name?: string;
//   indices_long_name: string;
//   indices_short_name: string;
// }

function App() {
  const [currentPage, setCurrentPage] = useState<Page>("landing");
  const [symbol, setSymbol] = useState("NIFTY");
  const [expiryDates, setExpiryDates] = useState<string[]>([]);
  const [selectedExpiry, setSelectedExpiry] = useState("");
  const [expiryLoading, setExpiryLoading] = useState(false);
  const [indexCards, setIndexCards] = useState<IndexCard[]>([]);
  const [cardsLoading, setCardsLoading] = useState(false);
  const [status, setStatus] = useState<StatusMessage>({
    state: "idle",
    message: "",
  });

  // Load index cards on landing page mount
  useEffect(() => {
    if (currentPage === "landing") {
      loadIndexCards();
      
      // Start the streamer when landing page is active
      invoke("start_streamer").catch(error => {
        console.error("Failed to start streamer:", error);
      });
      
      // Poll for updates every 1 second to get streaming data
      const interval = setInterval(() => {
        loadIndexCards();
      }, 1000);
      
      return () => {
        clearInterval(interval);
        // Stop the streamer when leaving landing page
        invoke("stop_streamer").catch(error => {
          console.error("Failed to stop streamer:", error);
        });
      };
    }
  }, [currentPage]);

  const loadIndexCards = async () => {
    setCardsLoading(true);
    try {
      const data = await invoke<IndexCard[]>("get_index_cards");
      console.log("Loaded index cards:", data);
      setIndexCards(data);
    } catch (error) {
      console.error("Failed to load index cards:", error);
      setStatus({
        state: "error",
        message: "Failed to load index cards",
      });
    } finally {
      setCardsLoading(false);
    }
  };

  // Fetch expiry dates when symbol changes
  useEffect(() => {
    if (currentPage === "detail") {
      const fetchExpiryDates = async () => {
        setExpiryLoading(true);
        try {
          const data = await invoke<ExpiryResponse>("fetch_expiry_dates", {
            symbol,
          });
          console.log("Received data from backend:", data);
          setExpiryDates(data.expiry_dates);
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
    }
  }, [symbol, currentPage]);

  const handleCardClick = (cardSymbol: string) => {
    setSymbol(cardSymbol);
    setCurrentPage("detail");
  };

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

  const formatPrice = (value: number) => {
    return value.toLocaleString('en-IN', {
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    });
  };

  const getArrow = (isPositive: boolean) => {
    return isPositive ? "▲" : "▼";
  };

  const getChangeColor = (isPositive: boolean) => {
    return isPositive ? "#27ae60" : "#e74c3c";
  };

  if (currentPage === "landing") {
    return (
      <main className="container landing">
        <h1>KSTOCKS</h1>
        
        <div className="cards-container">
          {cardsLoading ? (
            <div className="loading">Loading index cards...</div>
          ) : indexCards.length === 0 ? (
            <div className="error">No cards loaded</div>
          ) : (
            indexCards.map((card) => (
              <div
                key={card.index_name}
                className="index-card"
                onClick={() => handleCardClick(card.index_name)}
              >
                <div className="card-header">
                  <h3>{card.index_name}</h3>
                </div>
                <div className="card-body">
                  <div className="price-row">
                    <span className="price">{formatPrice(card.last_price)}</span>
                    <span
                      className="change"
                      style={{ color: getChangeColor(card.is_positive) }}
                    >
                      {getArrow(card.is_positive)} {formatPrice(Math.abs(card.change))} (
                      {formatPrice(card.change_percent)}%)
                    </span>
                  </div>
                  <div className="timestamp">
                    <small>Updated: {card.dissemination_time}</small>
                  </div>
                </div>
              </div>
            ))
          )}
        </div>

        {status.state !== "idle" && (
          <div className={`status-box status-${status.state}`}>
            <p>Status: {status.message}</p>
          </div>
        )}
      </main>
    );
  }

  return (
    <main className="container detail">
      <button className="back-button" onClick={() => setCurrentPage("landing")}>
        ← Back to Landing
      </button>

      <h1>Options Trading - {symbol}</h1>

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
          onChange={(e) => setSymbol(e.target.value)}
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