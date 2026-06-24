"use strict";
/* Spectrum control panel — talks to the Rust shell via withGlobalTauri invoke.
   Polls status() + recent_log() every 2s; buttons push control flags. */

const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args);
const $ = (id) => document.getElementById(id);

function ago(unix) {
  if (!unix) return "no poll yet";
  const s = Math.max(0, Math.floor(Date.now() / 1000) - unix);
  if (s < 60) return "last poll " + s + "s ago";
  if (s < 3600) return "last poll " + Math.floor(s / 60) + "m ago";
  return "last poll " + Math.floor(s / 3600) + "h ago";
}
function esc(s) {
  return String(s).replace(/[&<>]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;" }[c]));
}

async function refresh() {
  try {
    const v = await invoke("status");
    $("stateTxt").textContent = v.running ? "Running" : "Stopped";
    $("statePill").classList.toggle("on", !!v.running);
    const e = v.engine;
    if (e) {
      $("cFin").textContent = e.posted_financial;
      $("cPol").textContent = e.posted_political;
      $("cTec").textContent = e.posted_technology;
      $("cCat").textContent = e.posted_catastrophe;
      $("mSeen").textContent = e.seen;
      $("mQueued").textContent = e.queued;
      $("mFeeds").textContent = e.feeds;
      $("mPosted").textContent =
        e.posted_financial + e.posted_political + e.posted_technology + e.posted_catastrophe;
      $("lastPoll").textContent = ago(e.last_poll_unix);
    }
    const log = await invoke("recent_log");
    $("log").innerHTML = log
      .map((l) => `<div class="ln${/^posted/.test(l) ? " post" : ""}">${esc(l)}</div>`)
      .join("");
  } catch (err) {
    // running outside Tauri, or the engine is still starting — leave dashes.
  }
}

async function loadTuning() {
  try {
    const t = await invoke("get_tuning");
    $("tMin").value = t.min_severity;
    $("tPoll").value = t.poll_minutes;
    $("tDrip").value = t.drip_seconds;
    $("bands").innerHTML = (t.webhook_bands || [])
      .map((b) => `<span class="band">${esc(b)}</span>`)
      .join("");
  } catch (err) {}
}

$("btnStart").onclick = () => invoke("start").then(refresh);
$("btnStop").onclick = () => invoke("stop").then(refresh);
$("dry").onchange = (e) => invoke("set_dry", { dry: e.target.checked });
$("btnSave").onclick = async () => {
  await invoke("set_tuning", {
    tuning: {
      min_severity: Number($("tMin").value),
      poll_minutes: Number($("tPoll").value),
      drip_seconds: Number($("tDrip").value),
    },
  });
  const s = $("saved");
  s.hidden = false;
  setTimeout(() => (s.hidden = true), 1500);
  loadTuning();
};

loadTuning();
refresh();
setInterval(refresh, 2000);
