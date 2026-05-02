import { useMemo, useState, useEffect, type CSSProperties, type ReactNode } from "react";
import { AlbumManager } from "./components/AlbumManager/AlbumManager";
import { DeviceManager } from "./components/DeviceManager/DeviceManager";
import { FirmwareFlasher } from "./components/FirmwareFlasher/FirmwareFlasher";
import { StemCreator } from "./components/StemCreator/StemCreator";
import { useDeviceStore } from "./stores/deviceStore";
import "./App.css";

type TabId = "device" | "stems" | "albums" | "firmware";

const tabs: Array<{
  id: TabId;
  label: string;
  description: string;
  accent: string;
}> = [
  {
    id: "device",
    label: "Device",
    description: "USB status + eMMC",
    accent: "#a7f3d0",
  },
  {
    id: "stems",
    label: "Create",
    description: "YouTube → stems",
    accent: "#f9a8d4",
  },
  {
    id: "albums",
    label: "Albums",
    description: "Arrange + upload",
    accent: "#93c5fd",
  },
  {
    id: "firmware",
    label: "Firmware",
    description: "DFU flashing",
    accent: "#fde68a",
  },
];

function App() {
  const [activeTab, setActiveTab] = useState<TabId>("device");
  const [fwAutoStart, setFwAutoStart] = useState(false);
  const deviceInfo = useDeviceStore((s) => s.info);

  // Auto-switch to firmware tab when device needs update
  useEffect(() => {
    if (deviceInfo?.needs_firmware_update && activeTab === "device") {
      setFwAutoStart(true);
      setActiveTab("firmware");
    }
  }, [deviceInfo?.needs_firmware_update, activeTab]);

  const moduleMap: Record<TabId, ReactNode> = {
    device: <DeviceManager />,
    stems: <StemCreator />,
    albums: <AlbumManager />,
    firmware: <FirmwareFlasher autoStart={fwAutoStart} />,
  };
  const activeTabMeta = useMemo(
    () => tabs.find((tab) => tab.id === activeTab) ?? tabs[0],
    [activeTab],
  );

  return (
    <main className="app-shell">
      <div className="ambient ambient-one" />
      <div className="ambient ambient-two" />

      <aside className="side-nav" aria-label="Primary modules">
        <div className="brand-lockup">
          <div className="stem-disc" aria-hidden="true">
            <span />
          </div>
          <div>
            <p className="eyebrow">TE-StemPlayer</p>
            <h1>Studio</h1>
          </div>
        </div>

        <nav className="tab-stack">
          {tabs.map((tab) => (
            <button
              className={`tab-button ${activeTab === tab.id ? "tab-button--active" : ""}`}
              key={tab.id}
              onClick={() => setActiveTab(tab.id)}
              style={{ "--tab-accent": tab.accent } as CSSProperties}
            >
              <span className="tab-orb" />
              <span>
                <strong>{tab.label}</strong>
                <small>{tab.description}</small>
              </span>
            </button>
          ))}
        </nav>

        <div className="hardware-card">
          <p>Inspired by the Stem Player’s tactile disc controls.</p>
          <div className="mini-controls">
            <span />
          </div>
        </div>
      </aside>

      <section className="workspace">
        <header className="workspace-header">
          <div>
            <p className="eyebrow">Active module</p>
            <h2>{activeTabMeta.label}</h2>
            <span>{activeTabMeta.description}</span>
          </div>
          <div className="now-playing-disc" aria-hidden="true">
            <span />
          </div>
        </header>

        {moduleMap[activeTab]}
      </section>
    </main>
  );
}

export default App;
