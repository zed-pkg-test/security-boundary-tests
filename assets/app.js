import { validateApiPath, normalizeList, safeQuantity } from "./endpoint.js";

const mode = document.getElementById("mode");
const error = document.getElementById("error");
const kind = document.getElementById("kind");
const query = document.getElementById("query");
const rows = document.getElementById("rows");
const columns = document.getElementById("columns");
const empty = document.getElementById("empty");
const refresh = document.getElementById("refresh");

const apiParam = new URLSearchParams(location.search).get("api");

let data = { nodes: [], workloads: [] };
const text = (node, value) => { node.textContent = String(value ?? "—"); };
function element(tag, value, className) {
  const node = document.createElement(tag);
  node.textContent = String(value ?? "");
  if (className) node.className = className;
  return node;
}
function status(value) {
  const good = ["Ready", "Running", "Succeeded"].includes(value);
  const bad = ["Failed", "Unknown", "NotReady"].includes(value);
  return element("span", value || "Unknown", "pill " + (good ? "ok" : bad ? "bad" : ""));
}
function readableBytes(v) {
  return Number.isFinite(v) ? (v / 1073741824).toFixed(2) : "—";
}
function render() {
  const ready = data.nodes.filter(n => n.status?.phase === "Ready");
  const live = data.workloads.filter(w => w.status?.phase === "Running");
  text(document.getElementById("node-count"), ready.length + " / " + data.nodes.length);
  text(document.getElementById("workload-count"), live.length + " / " + data.workloads.length);
  const cpu = live.reduce((sum,w) => sum + (w.spec?.containers || []).reduce((n,c)=>n+safeQuantity(c.resources?.cpuMillis),0),0);
  const mem = live.reduce((sum,w) => sum + (w.spec?.containers || []).reduce((n,c)=>n+safeQuantity(c.resources?.memoryBytes),0),0);
  text(document.getElementById("cpu-used"), cpu.toLocaleString());
  text(document.getElementById("mem-used"), readableBytes(mem));

  const nodeMode = kind.value === "Node";
  const headers = nodeMode ? ["Name", "Zone", "Status", "CPU capacity", "Memory GiB"] :
                              ["Name", "Namespace", "Status", "Node", "Containers"];
  columns.replaceChildren(...headers.map(h=>element("th",h)));
  const values = (nodeMode ? data.nodes : data.workloads)
    .filter(r => [r.metadata?.name,r.metadata?.namespace].join(" ").toLowerCase().includes(query.value.trim().toLowerCase()));
  rows.replaceChildren(...values.map(r => {
    const tr = document.createElement("tr");
    const cells = nodeMode ?
      [r.metadata?.name,r.spec?.labels?.zone,r.status?.phase,r.spec?.capacity?.cpuMillis,
       readableBytes(r.spec?.capacity?.memoryBytes)] :
      [r.metadata?.name,r.metadata?.namespace,r.status?.phase,r.status?.nodeName,
       r.spec?.containers?.length || 0];
    cells.forEach((value,i) => {
      const td = document.createElement("td");
      if (i === 0) td.className = "name";
      if (i === 2) td.append(status(value));
      else td.textContent = String(value ?? "—");
      tr.append(td);
    });
    return tr;
  }));
  empty.hidden = values.length > 0;
}
async function load() {
  refresh.disabled = true;
  error.hidden = true;
  try {
    if (apiParam !== null) {
      const endpoint = new URL(validateApiPath(apiParam), location.origin);
      const fetchList = async name => {
        const url = new URL(endpoint);
        url.pathname = endpoint.pathname.replace(/\/$/, "") + "/v1alpha1/" + name;
        const response = await fetch(url, {headers: {Accept: "application/json"}, credentials: "same-origin"});
        if (!response.ok) throw new Error(name + " HTTP " + response.status);
        return normalizeList(await response.json());
      };
      const [nodes, workloads] = await Promise.all([fetchList("Node"),fetchList("Workload")]);
      data = {nodes,workloads};
      text(mode,"Live · proxied API");
      mode.classList.add("live");
    } else {
      const response = await fetch("./data/demo.json", {cache:"no-store"});
      if (!response.ok) throw new Error("Cannot load demonstration data");
      const payload = await response.json();
      data = {nodes: normalizeList(payload.nodes), workloads: normalizeList(payload.workloads)};
      text(mode,"Sample data · not live");
      mode.classList.remove("live");
    }
    render();
  } catch (e) {
    text(error,"Could not load resources: " + e.message);
    error.hidden = false;
    data = {nodes:[], workloads:[]};
    render();
  } finally {
    refresh.disabled = false;
  }
}
refresh.addEventListener("click", load);
kind.addEventListener("change", render);
query.addEventListener("input", render);
load();
