
import { useCallback, useEffect, useState } from "react";
import { tauriCommands, tauriEvents, type FlashProgressEvent } from "../../utils/tauriCommands";

type WizardStep = "detect" | "instructions" | "waiting" | "flashing" | "complete" | "error";

const STEP_LABELS: Record<WizardStep, string> = {
  detect: "Firmware Check",
  instructions: "Enter Bootloader Mode",
  waiting: "Waiting for Device",
  flashing: "Flashing Firmware",
  complete: "Update Complete",
  error: "Error",
};

export const FirmwareFlasher = ({ autoStart }: { autoStart?: boolean }) => {
  const [step, setStep] = useState<WizardStep>("detect");
  const [progress, setProgress] = useState<FlashProgressEvent | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [flashing, setFlashing] = useState(false);

  useEffect(() => {
    let mounted = true;
    const setup = async () => {
      const unlisteners = await Promise.all([
        tauriEvents.onFlashProgress((event) => {
          if (!mounted) return;
          setProgress(event);
          if (event.stage === "waiting_bootloader") {
            setStep("waiting");
          } else if (event.stage === "flashing") {
            setStep("flashing");
          } else if (event.stage === "complete") {
            setStep("complete");
            setFlashing(false);
          } else if (event.stage === "error") {
            setStep("error");
            setError(event.message);
            setFlashing(false);
          }
        }),
        tauriEvents.onFlashDone(() => {
          if (!mounted) return;
          setStep("complete");
          setFlashing(false);
        }),
      ]);
      return () => unlisteners.forEach((u) => u());
    };
    let cleanup: (() => void) | undefined;
    void setup().then((t) => { cleanup = t; });
    return () => { mounted = false; cleanup?.(); };
  }, []);

  useEffect(() => {
    if (autoStart && step === "detect") {
      setStep("instructions");
    }
  }, [autoStart, step]);

  const startFlash = useCallback(async () => {
    setFlashing(true);
    setError(null);
    setStep("waiting");
    try {
      await tauriCommands.startFirmwareFlash();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setStep("error");
      setFlashing(false);
    }
  }, []);

  const percent = progress?.percent ?? 0;

  return (
    <div className="module-grid">
      <section className="panel firmware-panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Firmware Update Wizard</p>
            <h2>{STEP_LABELS[step]}</h2>
          </div>
          <span className="round-badge round-badge--warning">DFU</span>
        </div>

        {/* Step indicator */}
        <div style={{ display: "flex", gap: "4px", margin: "12px 0 20px" }}>
          {(["instructions", "waiting", "flashing", "complete"] as WizardStep[]).map((s, i) => (
            <div
              key={s}
              style={{
                flex: 1,
                height: "4px",
                borderRadius: "2px",
                background:
                  step === s
                    ? "#fbbf24"
                    : ["instructions", "waiting", "flashing", "complete"].indexOf(step) > i
                      ? "#a7f3d0"
                      : "rgba(255,255,255,0.1)",
              }}
            />
          ))}
        </div>

        {step === "detect" && (
          <div className="warning-card">
            <strong>Your Stem Player needs a firmware update.</strong>
            <span>
              The current firmware does not support USB data transfers. We&apos;ll walk you through
              flashing custom firmware that enables full functionality.
            </span>
            <button
              className="primary-action wide"
              onClick={() => setStep("instructions")}
              style={{ marginTop: "16px" }}
            >
              Start Firmware Update
            </button>
          </div>
        )}

        {step === "instructions" && (
          <div>
            <div
              className="warning-card"
              style={{
                background: "rgba(250,204,21,0.08)",
                border: "1px solid rgba(250,204,21,0.2)",
              }}
            >
              <strong>⚠️ Do not disconnect during flashing.</strong>
              <span>The firmware binary (35 KB) has been pre-built for your nRF52840 MCU.</span>
            </div>

            <div style={{ margin: "20px 0", lineHeight: 1.8 }}>
              <h3 style={{ color: "#fbbf24", marginBottom: "12px" }}>
                Follow these steps exactly:
              </h3>
              <ol style={{ paddingLeft: "20px", color: "rgba(255,255,255,0.85)" }}>
                <li style={{ marginBottom: "12px" }}>
                  <strong>Unplug</strong> your Stem Player from USB
                </li>
                <li style={{ marginBottom: "12px" }}>
                  <strong>Press and hold</strong> the small <em>power/function button</em> on the
                  side edge of the Stem Player (it&apos;s a tiny recessed button, not the touch sliders)
                </li>
                <li style={{ marginBottom: "12px" }}>
                  <strong>While holding the side button</strong>, plug the USB-C cable back in
                </li>
                <li style={{ marginBottom: "12px" }}>
                  <strong>Keep holding</strong> for 3 seconds after plugging in, then release
                </li>
                <li>
                  The device should now be in <strong>bootloader mode</strong> — the LEDs may
                  behave differently and a new serial port will appear on your computer
                </li>
              </ol>
            </div>

            <button
              className="primary-action wide"
              onClick={() => void startFlash()}
              disabled={flashing}
              style={{ marginTop: "8px" }}
            >
              I&apos;m Ready — Start Flashing
            </button>
          </div>
        )}

        {step === "waiting" && (
          <div style={{ textAlign: "center", padding: "30px 0" }}>
            <div
              style={{
                width: "80px",
                height: "80px",
                margin: "0 auto 20px",
                border: "3px solid rgba(250,204,21,0.3)",
                borderTop: "3px solid #fbbf24",
                borderRadius: "50%",
                animation: "spin 1s linear infinite",
              }}
            />
            <style>{`@keyframes spin { to { transform: rotate(360deg); } }`}</style>
            <h3 style={{ color: "#fbbf24" }}>Waiting for Bootloader...</h3>
            <p style={{ color: "rgba(255,255,255,0.7)", maxWidth: "400px", margin: "12px auto" }}>
              {progress?.message ||
                "Unplug the device, hold the center button, and plug USB-C back in."}
            </p>
            <p style={{ color: "rgba(255,255,255,0.4)", fontSize: "0.8rem" }}>
              Scanning for serial port...
            </p>
          </div>
        )}

        {step === "flashing" && (
          <div style={{ textAlign: "center", padding: "30px 0" }}>
            <div
              className="flash-progress__ring"
              style={{
                width: "120px",
                height: "120px",
                margin: "0 auto 20px",
                borderRadius: "50%",
                background: `conic-gradient(#fbbf24 ${percent * 3.6}deg, rgba(255,255,255,0.08) 0deg)`,
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
              }}
            >
              <div
                style={{
                  width: "100px",
                  height: "100px",
                  borderRadius: "50%",
                  background: "#1a1a2e",
                  display: "flex",
                  flexDirection: "column",
                  alignItems: "center",
                  justifyContent: "center",
                }}
              >
                <strong style={{ fontSize: "1.5rem" }}>{percent}%</strong>
                <span style={{ fontSize: "0.75rem", opacity: 0.6 }}>flashing</span>
              </div>
            </div>
            <h3 style={{ color: "#a7f3d0" }}>Flashing Firmware...</h3>
            <p style={{ color: "rgba(255,255,255,0.7)" }}>
              {progress?.message || "Writing firmware to device..."}
            </p>
            <p
              style={{
                color: "#ef4444",
                fontWeight: "bold",
                fontSize: "0.85rem",
                marginTop: "12px",
              }}
            >
              ⚡ DO NOT unplug the device!
            </p>
          </div>
        )}

        {step === "complete" && (
          <div style={{ textAlign: "center", padding: "30px 0" }}>
            <div
              style={{
                width: "80px",
                height: "80px",
                margin: "0 auto 20px",
                borderRadius: "50%",
                background: "rgba(167,243,208,0.15)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                fontSize: "2.5rem",
              }}
            >
              ✅
            </div>
            <h3 style={{ color: "#a7f3d0" }}>Firmware Updated Successfully!</h3>
            <p style={{ color: "rgba(255,255,255,0.7)", maxWidth: "400px", margin: "12px auto" }}>
              {progress?.message ||
                "Press the function button on your Stem Player to restart it, then go to the Device tab and click Connect."}
            </p>
            <div style={{ marginTop: "20px" }}>
              <p style={{ color: "rgba(255,255,255,0.5)", fontSize: "0.85rem" }}>Next steps:</p>
              <ol
                style={{
                  textAlign: "left",
                  maxWidth: "350px",
                  margin: "8px auto",
                  color: "rgba(255,255,255,0.7)",
                  lineHeight: 1.8,
                }}
              >
                <li>Press the function button to restart the Stem Player</li>
                <li>Wait 5 seconds for it to boot</li>
                <li>Go to the <strong>Device</strong> tab and click <strong>Connect</strong></li>
              </ol>
            </div>
          </div>
        )}

        {step === "error" && (
          <div style={{ textAlign: "center", padding: "30px 0" }}>
            <div
              style={{
                width: "80px",
                height: "80px",
                margin: "0 auto 20px",
                borderRadius: "50%",
                background: "rgba(239,68,68,0.15)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                fontSize: "2.5rem",
              }}
            >
              ❌
            </div>
            <h3 style={{ color: "#ef4444" }}>Firmware Update Failed</h3>
            <p
              style={{
                color: "rgba(255,255,255,0.7)",
                maxWidth: "500px",
                margin: "12px auto",
                wordBreak: "break-word",
              }}
            >
              {error || "Unknown error occurred."}
            </p>
            <button
              className="primary-action"
              onClick={() => {
                setStep("instructions");
                setError(null);
                setProgress(null);
              }}
              style={{ marginTop: "16px" }}
            >
              Try Again
            </button>
          </div>
        )}

        {error && step !== "error" ? <p className="inline-error">{error}</p> : null}
      </section>
    </div>
  );
};
