import { useState, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";
import IndexChart from "./components/IndexChart";
import type { AppSettings } from "./types/settings";

type FetchStatus = "idle" | "loading" | "success" | "error";

interface StatusMessage {
  state: FetchStatus;
  message: string;
}

interface IndexCard {
  index_name: string;
  last_price: number;
  change: number;
  change_percent: number;
  is_positive: boolean;
  dissemination_time: string;
}

function App() {
  const [selectedCardIndex, setSelectedCardIndex] = useState<number>(0);
  const [expiryDates] = useState<string[]>([]);
  const [selectedExpiry, setSelectedExpiry] = useState("");
  const [expiryLoading] = useState(false);
  const [indexCards, setIndexCards] = useState<IndexCard[]>([]);
  const [cardsLoading, setCardsLoading] = useState(false);
  const [status, setStatus] = useState<StatusMessage>({
    state: "idle",
    message: "",
  });
  const [selectedTimeRange, setSelectedTimeRange] = useState<string>("1M");
  const cardsContainerRef = useRef<HTMLDivElement>(null);

  // Shallow-compare two IndexCard arrays (by content, not reference) so we
  // can skip setState when nothing has actually changed.
  const cardsEqual = (a: IndexCard[], b: IndexCard[]) => {
    if (a.length !== b.length) return false;
    for (let i = 0; i < a.length; i++) {
      const x = a[i];
      const y = b[i];
      if (
        x.index_name !== y.index_name ||
        x.last_price !== y.last_price ||
        x.change !== y.change ||
        x.change_percent !== y.change_percent ||
        x.is_positive !== y.is_positive ||
        x.dissemination_time !== y.dissemination_time
      ) {
        return false;
      }
    }
    return true;
  };

  // Full reload of all cards (used for initial load and the sparse fallback
  // poll). Real-time updates come via the "index-card-update" event instead.
  const loadIndexCards = useCallback(async () => {
    setCardsLoading(true);
    try {
      const data = await invoke<IndexCard[]>("get_index_cards");
      // Avoid replacing state (and triggering a re-render / scroll jump)
      // when the fetched data is identical to what's already shown.
      setIndexCards((prev) => (cardsEqual(prev, data) ? prev : data));
    } catch (error) {
      console.error("Failed to load index cards:", error);
      setStatus({
        state: "error",
        message: "Failed to load index cards",
      });
    } finally {
      setCardsLoading(false);
    }
  }, []);

  // Merge a single updated card into state by key, instead of replacing the
  // whole array. This avoids re-rendering cards that haven't changed.
  const mergeIndexCard = useCallback((updated: IndexCard) => {
    setIndexCards((prev) => {
      const idx = prev.findIndex((c) => c.index_name === updated.index_name);
      if (idx === -1) {
        return [...prev, updated];
      }
      const existing = prev[idx];
      // Skip the state update entirely if nothing actually changed.
      if (
        existing.last_price === updated.last_price &&
        existing.change === updated.change &&
        existing.change_percent === updated.change_percent &&
        existing.is_positive === updated.is_positive &&
        existing.dissemination_time === updated.dissemination_time
      ) {
        return prev;
      }
      const next = [...prev];
      next[idx] = updated;
      return next;
    });
  }, []);

  // Load index cards on component mount, subscribe to real-time push updates,
  // and keep a low-frequency fallback poll as a safety net.
  useEffect(() => {
    let fallbackIntervalId: ReturnType<typeof setInterval> | undefined;
    let unlistenCardUpdate: (() => void) | undefined;

    const setup = async () => {
      // Seed initial data.
      await loadIndexCards();

      // Start the streamer (emits "index-card-update" events as data changes).
      try {
        await invoke("start_streamer");
      } catch (error) {
        console.error("Failed to start streamer:", error);
      }

      // Subscribe to real-time card updates pushed from the backend.
      unlistenCardUpdate = await listen<IndexCard>(
        "index-card-update",
        (event) => {
          mergeIndexCard(event.payload);
        }
      );

      // Sparse fallback poll, purely as a safety net in case an event is
      // missed or the stream reconnects. Interval comes from settings.json.
      try {
        const settings = await invoke<AppSettings>("get_app_settings");
        fallbackIntervalId = setInterval(() => {
          loadIndexCards();
        }, settings.cards_fallback_poll_interval_seconds * 1000);
      } catch (error) {
        console.error("Failed to load app settings for fallback poll:", error);
      }
    };

    setup();

    return () => {
      if (fallbackIntervalId) clearInterval(fallbackIntervalId);
      if (unlistenCardUpdate) unlistenCardUpdate();
      // Stop the streamer when unmounting
      invoke("stop_streamer").catch((error) => {
        console.error("Failed to stop streamer:", error);
      });
    };
  }, [loadIndexCards, mergeIndexCard]);

  const handleCardClick = (index: number) => {
    setSelectedCardIndex(index);
    setSelectedTimeRange("1M"); // Reset to default time range
  };

  const scrollCards = (direction: "left" | "right") => {
    if (!cardsContainerRef.current) return;

    const scrollAmount = 300;
    const current = cardsContainerRef.current.scrollLeft;

    if (direction === "left") {
      cardsContainerRef.current.scrollLeft = current - scrollAmount;
    } else {
      cardsContainerRef.current.scrollLeft = current + scrollAmount;
    }
  };

  const canScrollLeft = cardsContainerRef.current
    ? cardsContainerRef.current.scrollLeft > 0
    : false;

  const canScrollRight = cardsContainerRef.current
    ? cardsContainerRef.current.scrollLeft <
      cardsContainerRef.current.scrollWidth -
        cardsContainerRef.current.clientWidth -
        10
    : false;

  async function fetch_data() {
    const selectedCard = indexCards[selectedCardIndex];
    if (!selectedCard) return;

    setStatus({ state: "loading", message: "Starting data fetch..." });

    try {
      const result = await invoke<string>("store_ticks", {
        symbol: selectedCard.index_name,
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
    return value.toLocaleString("en-IN", {
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

  const selectedCard = indexCards[selectedCardIndex];

  return (
    <main className="container landing">
      {/* <h1>KSTOCKS</h1> */}

      {/* Horizontal Card Carousel */}
      <div className="cards-wrapper">
        <button
          className="cards-navigation-button"
          onClick={() => scrollCards("left")}
          disabled={!canScrollLeft && selectedCardIndex === 0}
        >
          ‹
        </button>

        <div className="cards-container" ref={cardsContainerRef}>
          {cardsLoading ? (
            <div className="loading">Loading index cards...</div>
          ) : indexCards.length === 0 ? (
            <div className="error">No cards loaded</div>
          ) : (
            indexCards.map((card, index) => (
              <div
                key={card.index_name}
                className={`index-card ${
                  index === selectedCardIndex ? "active" : ""
                }`}
                onClick={() => handleCardClick(index)}
                style={{
                  opacity: index === selectedCardIndex ? 1 : 0.7,
                  transform:
                    index === selectedCardIndex ? "scale(1.05)" : "scale(1)",
                  borderColor:
                    index === selectedCardIndex ? "#396cd8" : "transparent",
                }}
              >
                <div className="card-header">
                  <h3>{card.index_name}</h3>
                </div>
                <div className="card-body">
                  <div className="price-row">
                    <span className="price">
                      {formatPrice(card.last_price)}
                    </span>
                    <span
                      className="change"
                      style={{ color: getChangeColor(card.is_positive) }}
                    >
                      {getArrow(card.is_positive)}{" "}
                      {formatPrice(Math.abs(card.change))} (
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

        <button
          className="cards-navigation-button"
          onClick={() => scrollCards("right")}
          disabled={!canScrollRight && selectedCardIndex === indexCards.length - 1}
        >
          ›
        </button>
      </div>

      {/* Detail Section */}
      {selectedCard && (
        <div className="detail-section">
          <h2>{selectedCard.index_name} Details</h2>

          {/* Time Range Selector */}
          <div className="time-range-selector">
            {[
              "1D", "1M", "3M", "6M", "1Y", "5Y", "10Y", "15Y", "20Y", "25Y",
              "30Y",
            ].map((flag) => (
              <button
                key={flag}
                className={`time-range-button ${
                  selectedTimeRange === flag ? "active" : ""
                }`}
                onClick={() => setSelectedTimeRange(flag)}
              >
                {flag}
              </button>
            ))}
          </div>

          {/* Insights */}
          <div className="insights-container">
            <div className="insight-item">
              <div className="insight-label">Current Price</div>
              <div className="insight-value">
                {formatPrice(selectedCard.last_price)}
              </div>
            </div>
            <div className="insight-item">
              <div className="insight-label">Change</div>
              <div
                className="insight-value"
                style={{ color: getChangeColor(selectedCard.is_positive) }}
              >
                {getArrow(selectedCard.is_positive)}{" "}
                {formatPrice(Math.abs(selectedCard.change))}
              </div>
            </div>
            <div className="insight-item">
              <div className="insight-label">Change %</div>
              <div
                className="insight-value"
                style={{ color: getChangeColor(selectedCard.is_positive) }}
              >
                {formatPrice(selectedCard.change_percent)}%
              </div>
            </div>
            <div className="insight-item">
              <div className="insight-label">Updated</div>
              <div className="insight-value" style={{ fontSize: "0.9em" }}>
                {selectedCard.dissemination_time}
              </div>
            </div>
          </div>

          {/* Chart */}
          <IndexChart
            index_display_name={selectedCard.index_name}
            time_range_flag={selectedTimeRange}
          />
        </div>
      )}

      {/* Options Fetch Section */}
      {selectedCard && (
        <form
          className="row"
          onSubmit={(e) => {
            e.preventDefault();
            fetch_data();
          }}
        >
          <select
            id="expiry"
            value={selectedExpiry}
            onChange={(e) => setSelectedExpiry(e.target.value)}
            disabled={
              status.state === "loading" ||
              expiryLoading ||
              expiryDates.length === 0
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
            {status.state === "loading" ? "Fetching..." : "Fetch Options Data"}
          </button>
        </form>
      )}

      {status.state !== "idle" && (
        <div className={`status-box status-${status.state}`}>
          <p>Status: {status.message}</p>
        </div>
      )}
    </main>
  );
}

export default App;