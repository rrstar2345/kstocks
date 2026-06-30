import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
} from "recharts";

interface ChartDataPoint {
  timestamp: number;
  price: number;
  status: string;
  change: number;
  change_percent: number;
}

interface IndexChartProps {
  index_display_name: string;
  time_range_flag: string;
}

interface ChartPoint {
  timestamp: number;
  price: number;
  time: string;
  isPreOpen: boolean;
  originalData: ChartDataPoint;
}

interface ChartConfig {
  index_chart_refresh_interval_seconds: number;
}

const IndexChart: React.FC<IndexChartProps> = ({
  index_display_name,
  time_range_flag,
}) => {
  const [chartData, setChartData] = useState<ChartPoint[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [lineColor, setLineColor] = useState<string>("#8884d8");

  const formatTimeDisplay = (timestamp: number, is_one_day: boolean): string => {
    const date = new Date(timestamp);

    if (is_one_day) {
      // For 1D: show time only (HH:MM)
      return date.toLocaleTimeString("en-IN", {
        hour: "2-digit",
        minute: "2-digit",
        hour12: true,
      });
    } else {
      // For other flags: show date (DD/MM/YYYY)
      return date.toLocaleDateString("en-IN");
    }
  };

  useEffect(() => {
    fetchChartData();

    // Set up refresh interval
    const config = getConfig();
    let intervalId: ReturnType<typeof setInterval> | undefined;

    const setupInterval = async () => {
      try {
        const cfg = await config;
        intervalId = setInterval(() => {
          fetchChartData();
        }, cfg.index_chart_refresh_interval_seconds * 1000);
      } catch (error) {
        console.error("Failed to get config for refresh interval:", error);
      }
    };

    setupInterval();

    return () => {
      if (intervalId) clearInterval(intervalId);
    };
  }, [index_display_name, time_range_flag]);

  const getConfig = async () => {
    // This is a placeholder - the config is fetched from the backend
    // For now, return default
    return {
      index_chart_refresh_interval_seconds: 15,
    } as ChartConfig;
  };

  const fetchChartData = async () => {
    setLoading(true);
    setError(null);

    try {
      const rawData = await invoke<ChartDataPoint[]>(
        "get_index_chart_data",
        {
          indexDisplayName: index_display_name,
          timeRangeFlag: time_range_flag,
        }
      );

      // Process the data
      if (rawData && rawData.length > 0) {
        const is_one_day = time_range_flag === "1D";
        const processedData = rawData.map((point) => ({
          timestamp: point.timestamp,
          price: point.price,
          time: formatTimeDisplay(point.timestamp, is_one_day),
          isPreOpen: point.status === "PO",
          originalData: point,
        }));

        setChartData(processedData);

        // Determine line color based on last price vs previous close
        if (processedData.length > 0) {
          const lastPrice = processedData[processedData.length - 1].price;
          const firstPrice = processedData[0].price;
          const color = lastPrice >= firstPrice ? "#27ae60" : "#e74c3c";
          setLineColor(color);
        }
      } else {
        setChartData([]);
      }
    } catch (err) {
      const errorMsg = err instanceof Error ? err.message : String(err);
      setError(`Failed to load chart data: ${errorMsg}`);
      console.error("Chart fetch error:", err);
    } finally {
      setLoading(false);
    }
  };

  const renderTooltip = (props: any) => {
    const { active, payload } = props;

    if (active && payload && payload.length > 0) {
      const data = payload[0].payload as ChartPoint;
      const is_one_day = time_range_flag === "1D";

      return (
        <div
          style={{
            backgroundColor: "#fff",
            padding: "8px",
            borderRadius: "4px",
            border: "1px solid #ccc",
          }}
        >
          <p style={{ margin: "0 0 4px 0" }}>
            <strong>Price:</strong> ₹{data.price.toFixed(2)}
          </p>
          <p style={{ margin: "0 0 4px 0" }}>
            <strong>{is_one_day ? "Time" : "Date"}:</strong> {data.time}
          </p>
          {is_one_day && (
            <>
              <p style={{ margin: "0 0 4px 0" }}>
                <strong>Change:</strong> {data.originalData.change.toFixed(2)}
              </p>
              <p style={{ margin: "0" }}>
                <strong>Change %:</strong>{" "}
                {data.originalData.change_percent.toFixed(2)}%
              </p>
            </>
          )}
          {data.isPreOpen && (
            <p style={{ margin: "4px 0 0 0", color: "#ff9800" }}>
              <strong>Pre-Open</strong>
            </p>
          )}
        </div>
      );
    }

    return null;
  };

  if (loading && chartData.length === 0) {
    return <div style={{ textAlign: "center", padding: "40px" }}>Loading chart...</div>;
  }

  if (error) {
    return (
      <div style={{ textAlign: "center", padding: "40px", color: "#e74c3c" }}>
        {error}
      </div>
    );
  }

  if (chartData.length === 0) {
    return (
      <div style={{ textAlign: "center", padding: "40px" }}>
        No data available for {time_range_flag}
      </div>
    );
  }

  return (
    <div className="chart-container">
      <ResponsiveContainer width="100%" height="100%">
        <LineChart
          data={chartData}
          margin={{ top: 5, right: 30, left: 0, bottom: 5 }}
        >
          <CartesianGrid strokeDasharray="3 3" />
          <XAxis
            dataKey="time"
            angle={-45}
            textAnchor="end"
            height={80}
            tick={{ fontSize: 12 }}
          />
          <YAxis
            tick={{ fontSize: 12 }}
            domain={["dataMin - 100", "dataMax + 100"]}
          />
          <Tooltip content={renderTooltip} />
          <Line
            type="monotone"
            dataKey="price"
            stroke={lineColor}
            dot={false}
            isAnimationActive={false}
            strokeWidth={2}
          />
        </LineChart>
      </ResponsiveContainer>
    </div>
  );
};

export default IndexChart;