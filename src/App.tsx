import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

function App() {

  const [symb, setSymb] = useState("NIFTY");
  const [status, setStatus] = useState(null);
  
  async function fetch_data() {
    // Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
    setStatus(await invoke("store_ticks", { symb }));
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
        <select id="symb" value={symb} onChange={(e) => setSymb(e.target.value)}>
          <option value="NIFTY">NIFTY 50</option>
          <option value="NIFTYNXT50">NIFTY Next 50</option>
          <option value="FINNIFTY">FIN NIFTY</option>
          <option value="BANKNIFTY">BANK NIFTY</option>
          <option value="MIDCPNIFTY">MIDCAP NIFTY</option>
        </select>
        <button type="submit">Fetch</button>
      </form>
      <p>Selected Symbol: {symb}</p>
      <p>Status: {status}</p>
    </main>
  );
}

export default App;
