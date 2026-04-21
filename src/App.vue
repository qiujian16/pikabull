<script setup lang="ts">
import { ref, reactive, watch, nextTick, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { marked } from "marked";
import Plotly from "plotly.js-dist-min";
import BacktestView from "./BacktestView.vue";

interface StockResult {
  code: string;
  name: string;
}

interface StepState {
  status: "pending" | "running" | "done";
  content: string;
  streaming: boolean;
}

interface StockCard {
  symbol: string;
  name: string;
  decision: string;
  steps: Record<string, StepState>;
  chartData: any;
}

interface ModelConfig {
  id: string;
  name: string;
  provider: string;
  model: string;
  apiKey: string;
  baseUrl: string;
}

interface MarketIndex {
  code: string;
  name: string;
  price: number;
  change: number;
  changePct: number;
  volume: number;
  turnover: number;
  high: number;
  low: number;
  open: number;
  prevClose: number;
}

interface WatchlistItem {
  symbol: string;
  name: string;
  sortOrder: number;
  addedAt: string;
}

interface StockQuote {
  code: string;
  name: string;
  price: number;
  change: number;
  changePct: number;
}

interface ReportMeta {
  symbol: string;
  decision: string;
  analyzedAt: string;
}

interface SavedReport {
  symbol: string;
  name: string;
  startDate: string;
  endDate: string;
  decision: string;
  chartData: string | null;
  reportData: string;
  analyzedAt: string;
}

const STEP_ORDER = [
  "chart",
  "market",
  "fundamental",
  "news",
  "bull",
  "bear",
  "trader",
  "risk",
];
const STEP_LABELS: Record<string, { icon: string; label: string }> = {
  chart: { icon: "bar-chart-line", label: "图表生成" },
  market: { icon: "activity", label: "技术分析" },
  fundamental: { icon: "building", label: "基本面分析" },
  news: { icon: "newspaper", label: "新闻资讯" },
  bull: { icon: "arrow-up-circle", label: "🐂 多方论据" },
  bear: { icon: "arrow-down-circle", label: "🐻 空方论据" },
  trader: { icon: "person-badge", label: "交易决策" },
  risk: { icon: "shield-check", label: "风险评估" },
};

const searchQuery = ref("");
const searchResults = ref<StockResult[]>([]);
const showDropdown = ref(false);
const startDate = ref("");
const endDate = ref("");
const runningSymbols = reactive(new Set<string>());
const analysisTarget = ref<{ symbol: string; name: string } | null>(null);
const TOGGLEABLE_AGENTS = ["market", "fundamental", "news", "bull", "bear"];
const enabledAgents = reactive(new Set(TOGGLEABLE_AGENTS));
const cards = reactive(new Map<string, StockCard>());
const expandedSections = reactive(new Set<string>());
const providerInfo = ref({ provider: "", model: "" });
const currentView = ref<"home" | "analysis" | "backtest">("home");
const marketIndices = ref<MarketIndex[]>([]);
const watchlist = ref<WatchlistItem[]>([]);
const watchlistQuotes = reactive(new Map<string, StockQuote>());
const reportMetas = reactive(new Map<string, ReportMeta>());
const errorMessage = ref("");
const showConfigModal = ref(false);
const modelConfigs = ref<ModelConfig[]>([]);
const activeModelId = ref("");
const configFormMode = ref<"list" | "add" | "edit">("list");
const configForm = reactive({
  id: "",
  name: "",
  provider: "anthropic",
  model: "",
  apiKey: "",
  baseUrl: "",
});

let searchTimer: ReturnType<typeof setTimeout> | null = null;
let indicesTimer: ReturnType<typeof setInterval> | null = null;

onMounted(async () => {
  setRange(180);
  document.addEventListener('click', onClickOutside);
  await loadConfigs();
  await loadWatchlist();
  fetchMarketIndices();
  fetchWatchlistQuotes();

  // Retry quickly if initial load got empty data (backend may still be starting)
  setTimeout(() => {
    if (marketIndices.value.length === 0) fetchMarketIndices();
    if (watchlist.value.length > 0 && watchlistQuotes.size === 0) fetchWatchlistQuotes();
  }, 2000);
  setTimeout(() => {
    if (marketIndices.value.length === 0) fetchMarketIndices();
    if (watchlist.value.length > 0 && watchlistQuotes.size === 0) fetchWatchlistQuotes();
  }, 5000);

  indicesTimer = setInterval(() => {
    fetchMarketIndices();
    fetchWatchlistQuotes();
  }, 15000);

  await listen<any>("analysis-event", (event) => {
    handleEvent(event.payload);
  });
});

const chartObservers = new Map<string, ResizeObserver>();

watch(
  [() => currentView.value, () => analysisTarget.value?.symbol],
  async () => {
    if (currentView.value === 'analysis' && analysisTarget.value) {
      await nextTick();
      const symbol = analysisTarget.value.symbol;
      const card = cards.get(symbol);
      const el = document.getElementById(`chart-${symbol}`);
      if (el && card?.chartData) {
        const layout = { ...card.chartData.layout, autosize: true };
        delete layout.height;
        delete layout.width;
        await Plotly.newPlot(el, card.chartData.data, layout, {
          responsive: true,
          displayModeBar: false,
        });
      }
    }
  }
);

onUnmounted(() => {
  document.removeEventListener('click', onClickOutside);
  for (const obs of chartObservers.values()) obs.disconnect();
  chartObservers.clear();
  if (indicesTimer) clearInterval(indicesTimer);
});

function setRange(days: number) {
  const end = new Date();
  const start = new Date();
  start.setDate(end.getDate() - days);
  startDate.value = toISO(start);
  endDate.value = toISO(end);
}

function toISO(d: Date): string {
  return d.toISOString().split("T")[0];
}

async function doSearch() {
  const q = searchQuery.value.trim();
  if (!q) {
    showDropdown.value = false;
    return;
  }
  try {
    searchResults.value = await invoke("search_stocks", { query: q });
    showDropdown.value = searchResults.value.length > 0;
  } catch {
    showDropdown.value = false;
  }
}

function onSearchInput() {
  if (searchTimer) clearTimeout(searchTimer);
  searchTimer = setTimeout(doSearch, 200);
}

function onClickOutside(e: MouseEvent) {
  const wrapper = document.querySelector('.search-wrapper');
  if (wrapper && !wrapper.contains(e.target as Node)) {
    showDropdown.value = false;
  }
}

function addStock(code: string, name: string) {
  showDropdown.value = false;
  searchQuery.value = "";
  analysisTarget.value = { symbol: code, name };
  currentView.value = "analysis";
}

async function startCurrentAnalysis() {
  if (!analysisTarget.value) return;
  const { symbol } = analysisTarget.value;
  runningSymbols.add(symbol);

  try {
    await invoke("start_analysis", {
      symbols: [symbol],
      startDate: startDate.value,
      endDate: endDate.value,
      enabledAgents: [...enabledAgents, "trader", "risk"],
    });
  } catch (e: any) {
    errorMessage.value = String(e);
    runningSymbols.delete(symbol);
  }
}

function handleEvent(data: any) {
  switch (data.type) {
    case "start":
      createCard(data);
      break;
    case "step_start":
      markStepRunning(data.symbol, data.step);
      break;
    case "chart":
      renderChart(data);
      break;
    case "chunk":
      renderChunk(data);
      break;
    case "report":
      renderReport(data);
      break;
    case "final":
      renderFinal(data);
      break;
    case "symbol-done":
      runningSymbols.delete(data.symbol);
      loadReportMetas();
      break;
    case "error":
      console.error("Analysis error:", data.message);
      break;
  }
}

function createCard(data: any) {
  const steps: Record<string, StepState> = {};
  for (const s of STEP_ORDER) {
    steps[s] = { status: "pending", content: "", streaming: false };
  }
  cards.set(data.symbol, {
    symbol: data.symbol,
    name: data.name,
    decision: "",
    steps,
    chartData: null,
  });
}

function markStepRunning(symbol: string, step: string) {
  const card = cards.get(symbol);
  if (card?.steps[step]) {
    card.steps[step].status = "running";
  }
}

function markStepDone(symbol: string, step: string) {
  const card = cards.get(symbol);
  if (card?.steps[step]) {
    card.steps[step].status = "done";
  }
}

async function renderChart(data: any) {
  markStepDone(data.symbol, "chart");
  const card = cards.get(data.symbol);
  if (!card || !data.chart) return;
  card.chartData = data.chart;

  await nextTick();
  const el = document.getElementById(`chart-${data.symbol}`);
  if (el && data.chart) {
    const layout = {
      ...data.chart.layout,
      autosize: true,
      height: undefined,
      width: undefined,
    };
    delete layout.height;
    delete layout.width;

    await Plotly.newPlot(el, data.chart.data, layout, {
      responsive: true,
      displayModeBar: false,
    });

    // Use ResizeObserver for robust resize handling
    const oldObs = chartObservers.get(data.symbol);
    if (oldObs) oldObs.disconnect();

    let debounce: ReturnType<typeof setTimeout> | null = null;
    const obs = new ResizeObserver(() => {
      if (debounce) clearTimeout(debounce);
      debounce = setTimeout(() => {
        Plotly.Plots.resize(el);
      }, 100);
    });
    obs.observe(el);
    chartObservers.set(data.symbol, obs);
  }
}

function renderChunk(data: any) {
  const card = cards.get(data.symbol);
  if (!card?.steps[data.step]) return;
  card.steps[data.step].streaming = true;
  card.steps[data.step].content += data.text;
  expandedSections.add(`${data.symbol}-${data.step}`);
}

function renderReport(data: any) {
  const card = cards.get(data.symbol);
  if (!card?.steps[data.step]) return;
  card.steps[data.step].content = data.content || "";
  card.steps[data.step].streaming = false;
  markStepDone(data.symbol, data.step);
}

function renderFinal(data: any) {
  const card = cards.get(data.symbol);
  if (card) {
    card.decision = data.decision;

    const rd: Record<string, string> = {};
    for (const step of STEP_ORDER) {
      if (card.steps[step]?.content) rd[step] = card.steps[step].content;
    }
    invoke("save_analysis_report", {
      symbol: card.symbol,
      name: card.name,
      startDate: startDate.value,
      endDate: endDate.value,
      decision: data.decision,
      chartData: card.chartData ? JSON.stringify(card.chartData) : null,
      reportData: JSON.stringify(rd),
    }).then(() => loadReportMetas());
  }
}

function toggleSection(key: string) {
  if (expandedSections.has(key)) {
    expandedSections.delete(key);
  } else {
    expandedSections.add(key);
  }
}

function stripThink(text: string): string {
  let result = text.replace(/<think>[\s\S]*?<\/think>\n?/g, "");
  result = result.replace(/<think>[\s\S]*$/g, "");
  return result.trimStart();
}

function renderMarkdown(text: string): string {
  return marked.parse(stripThink(text)) as string;
}

function decisionLabel(decision: string): string {
  const labels: Record<string, string> = {
    BUY: "买入 ▲",
    HOLD: "持有 —",
    SELL: "卖出 ▼",
  };
  return labels[decision] || decision || "分析中…";
}

function providerIcon(provider: string): string {
  const icons: Record<string, string> = {
    anthropic: "🟠",
    openai: "🟢",
    ollama: "🔵",
    minimax: "🟣",
  };
  return icons[provider] || "⚪";
}

async function fetchMarketIndices() {
  try {
    const data: MarketIndex[] = await invoke("get_market_indices");
    if (data.length > 0) marketIndices.value = data;
  } catch {}
}

function goHome() {
  currentView.value = "home";
  fetchWatchlistQuotes();
}

function formatTurnover(val: number): string {
  if (val >= 1e8) return (val / 1e8).toFixed(2) + "亿";
  if (val >= 1e4) return (val / 1e4).toFixed(2) + "万";
  return val.toFixed(2);
}

// ── Watchlist ──

async function loadWatchlist() {
  try {
    watchlist.value = await invoke("get_watchlist");
    await loadReportMetas();
  } catch {}
}

async function loadReportMetas() {
  try {
    const metas: ReportMeta[] = await invoke("list_report_metas");
    reportMetas.clear();
    for (const m of metas) reportMetas.set(m.symbol, m);
  } catch {}
}

async function viewSavedReport(symbol: string) {
  try {
    const report: SavedReport | null = await invoke("get_saved_report", { symbol });
    if (!report) return;

    const rd = JSON.parse(report.reportData) as Record<string, string>;
    const steps: Record<string, StepState> = {};
    for (const s of STEP_ORDER) {
      steps[s] = { status: rd[s] ? "done" : "pending", content: rd[s] || "", streaming: false };
    }

    cards.clear();
    expandedSections.clear();
    cards.set(symbol, {
      symbol: report.symbol,
      name: report.name,
      decision: report.decision,
      steps,
      chartData: report.chartData ? JSON.parse(report.chartData) : null,
    });

    for (const s of STEP_ORDER) {
      if (rd[s]) expandedSections.add(`${symbol}-${s}`);
    }

    analysisTarget.value = { symbol: report.symbol, name: report.name };
    currentView.value = "analysis";

    if (report.chartData) {
      await nextTick();
      const chartObj = JSON.parse(report.chartData);
      const el = document.getElementById(`chart-${symbol}`);
      if (el) {
        const layout = { ...chartObj.layout, autosize: true };
        delete layout.height;
        delete layout.width;
        await Plotly.newPlot(el, chartObj.data, layout, { responsive: true, displayModeBar: false });
      }
    }
  } catch (e: any) {
    errorMessage.value = "加载报告失败: " + String(e);
  }
}

function timeAgo(isoStr: string): string {
  const d = new Date(isoStr);
  const now = Date.now();
  const diff = now - d.getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return "刚刚";
  if (mins < 60) return `${mins}分钟前`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}小时前`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}天前`;
  return d.toLocaleDateString();
}

async function fetchWatchlistQuotes() {
  if (watchlist.value.length === 0) return;
  try {
    const symbols: string[] = watchlist.value.map((w) => w.symbol);
    const quotes: StockQuote[] = await invoke("get_watchlist_quotes", { symbols });
    for (const q of quotes) watchlistQuotes.set(q.code, q);
  } catch {}
}

async function addToWatchlist(symbol: string, name: string) {
  await invoke("add_to_watchlist", { symbol, name });
  await loadWatchlist();
  fetchWatchlistQuotes();
}

async function removeFromWatchlist(symbol: string) {
  await invoke("remove_from_watchlist", { symbol });
  await loadWatchlist();
}

function isInWatchlist(symbol: string): boolean {
  return watchlist.value.some((w) => w.symbol === symbol);
}

function analyzeWatchlistStock(symbol: string) {
  const item = watchlist.value.find(w => w.symbol === symbol);
  analysisTarget.value = { symbol, name: item?.name || symbol };
  currentView.value = "analysis";
}

function toggleAgent(step: string) {
  if (enabledAgents.has(step)) {
    enabledAgents.delete(step);
  } else {
    enabledAgents.add(step);
  }
}

function isAnalyzing(): boolean {
  return !!analysisTarget.value && runningSymbols.has(analysisTarget.value.symbol);
}

function maskApiKey(key: string): string {
  if (!key) return "";
  if (key.length <= 8) return "••••••••";
  return key.slice(0, 4) + "••••" + key.slice(-4);
}

async function loadConfigs() {
  try {
    modelConfigs.value = await invoke("list_model_configs");
    const active = await invoke<ModelConfig | null>("get_active_model");
    if (active) {
      activeModelId.value = active.id;
      providerInfo.value = { provider: active.provider, model: active.model };
    } else {
      activeModelId.value = "";
      try {
        providerInfo.value = await invoke("get_provider_info");
      } catch {
        providerInfo.value = { provider: "", model: "" };
      }
    }
  } catch {}
}

function openConfigModal() {
  configFormMode.value = "list";
  showConfigModal.value = true;
}

function startAddConfig() {
  configError.value = "";
  configForm.id = "";
  configForm.name = "";
  configForm.provider = "anthropic";
  configForm.model = "";
  configForm.apiKey = "";
  configForm.baseUrl = "";
  configFormMode.value = "add";
}

function startEditConfig(config: ModelConfig) {
  configError.value = "";
  configForm.id = config.id;
  configForm.name = config.name;
  configForm.provider = config.provider;
  configForm.model = config.model;
  configForm.apiKey = config.apiKey;
  configForm.baseUrl = config.baseUrl;
  configFormMode.value = "edit";
}

function cancelConfigForm() {
  configFormMode.value = "list";
}

const configError = ref("");

async function saveConfig() {
  configError.value = "";

  if (!configForm.name.trim()) {
    configError.value = "请输入名称";
    return;
  }
  if (!configForm.model.trim()) {
    configError.value = "请输入模型名称";
    return;
  }

  try {
    if (configFormMode.value === "add") {
      await invoke("add_model_config", {
        name: configForm.name,
        provider: configForm.provider,
        model: configForm.model,
        apiKey: configForm.apiKey,
        baseUrl: configForm.baseUrl,
      });
    } else {
      await invoke("update_model_config", {
        id: configForm.id,
        name: configForm.name,
        provider: configForm.provider,
        model: configForm.model,
        apiKey: configForm.apiKey,
        baseUrl: configForm.baseUrl,
      });
    }

    await loadConfigs();
    configFormMode.value = "list";
  } catch (e: any) {
    configError.value = "保存失败: " + String(e);
  }
}

async function deleteModelConfig(id: string) {
  await invoke("delete_model_config", { id });
  await loadConfigs();
}

async function setActiveModel(id: string) {
  await invoke("set_active_model", { id });
  await loadConfigs();
}
</script>

<template>
  <div class="app-container">
    <!-- Sidebar -->
    <div v-if="currentView !== 'backtest'" class="sidebar">
      <div class="brand">⚡🐂 PikaBull</div>
      <div class="provider-badge" @click="openConfigModal" title="点击配置模型">
        <template v-if="providerInfo.provider">
          {{ providerIcon(providerInfo.provider) }}
          {{ providerInfo.provider }} · {{ providerInfo.model }}
        </template>
        <template v-else>⚙ 点击配置模型</template>
        <span class="config-gear">⚙</span>
      </div>

      <!-- Nav buttons -->
      <div v-if="currentView === 'home'" class="nav-buttons">
        <button class="nav-btn" @click="currentView = 'backtest'">
          回测系统
        </button>
      </div>

      <!-- Stock search (home only) -->
      <template v-if="currentView === 'home'">
        <div class="section">
          <label class="section-label">股票搜索</label>
          <div class="search-wrapper">
            <input
              v-model="searchQuery"
              @input="onSearchInput"
              @focus="() => { if (searchQuery) doSearch(); }"
              placeholder="输入代码或名称…"
              class="search-input"
            />
            <div v-if="showDropdown" class="search-dropdown" @mousedown.prevent>
              <div
                v-for="s in searchResults"
                :key="s.code"
                class="dropdown-item"
                @click="addStock(s.code, s.name)"
                @pointerdown.prevent="addStock(s.code, s.name)"
              >
                <span style="flex:1;cursor:pointer">
                  {{ s.name }}
                  <span class="code">{{ s.code }}</span>
                </span>
                <span
                  class="watchlist-star"
                  :class="{ starred: isInWatchlist(s.code) }"
                  @click.stop="isInWatchlist(s.code) ? removeFromWatchlist(s.code) : addToWatchlist(s.code, s.name)"
                  @pointerdown.prevent.stop="isInWatchlist(s.code) ? removeFromWatchlist(s.code) : addToWatchlist(s.code, s.name)"
                  :title="isInWatchlist(s.code) ? '取消关注' : '加入自选'"
                >{{ isInWatchlist(s.code) ? '★' : '☆' }}</span>
              </div>
            </div>
          </div>
        </div>
      </template>

      <!-- Analysis page controls -->
      <template v-if="currentView === 'analysis'">
        <!-- Selected stock -->
        <div v-if="analysisTarget" class="section">
          <label class="section-label">当前股票</label>
          <div class="analysis-target">
            <div class="target-info">
              <span class="target-name">{{ analysisTarget.name }}</span>
              <span class="target-code">{{ analysisTarget.symbol }}</span>
            </div>
            <span
              class="watchlist-star"
              :class="{ starred: isInWatchlist(analysisTarget.symbol) }"
              @click="isInWatchlist(analysisTarget.symbol) ? removeFromWatchlist(analysisTarget.symbol) : addToWatchlist(analysisTarget.symbol, analysisTarget.name)"
            >{{ isInWatchlist(analysisTarget.symbol) ? '★' : '☆' }}</span>
          </div>
        </div>

        <!-- Date range -->
        <div class="section">
          <label class="section-label">分析区间</label>
          <div class="date-inputs">
            <input v-model="startDate" type="date" class="date-input" />
            <input v-model="endDate" type="date" class="date-input" />
          </div>
          <div class="range-buttons">
            <button @click="setRange(90)">3个月</button>
            <button @click="setRange(180)">6个月</button>
            <button @click="setRange(365)">1年</button>
          </div>
        </div>

        <!-- Agent toggles -->
        <div class="section">
          <label class="section-label">分析模块</label>
          <div class="agent-toggles">
            <label
              v-for="step in TOGGLEABLE_AGENTS"
              :key="step"
              class="agent-toggle"
            >
              <input
                type="checkbox"
                :checked="enabledAgents.has(step)"
                @change="toggleAgent(step)"
              />
              <span>{{ STEP_LABELS[step]?.label || step }}</span>
            </label>
          </div>
        </div>

        <!-- Start button -->
        <div class="sidebar-footer">
          <button
            class="btn-primary"
            @click="startCurrentAnalysis"
            :disabled="!analysisTarget || isAnalyzing()"
          >
            {{ isAnalyzing() ? '⟳ 分析中…' : '▶ 开始分析' }}
          </button>
        </div>
      </template>
    </div>

    <!-- Backtest view (full viewport, own sidebar) -->
    <BacktestView
      v-if="currentView === 'backtest'"
      @go-home="goHome"
    />

    <!-- Main content -->
    <div v-if="currentView !== 'backtest'" class="main-content">
      <div v-if="errorMessage" class="error-banner" @click="errorMessage = ''">
        {{ errorMessage }}
        <small style="display:block;margin-top:4px;opacity:0.7">点击关闭</small>
      </div>

      <!-- Home view -->
      <div v-show="currentView === 'home'" class="home-view">
        <h2 class="home-title">A股大盘</h2>
        <div class="index-grid">
          <div
            v-for="idx in marketIndices"
            :key="idx.code"
            class="index-card"
            :class="idx.changePct >= 0 ? 'idx-up' : 'idx-down'"
          >
            <div class="idx-name">{{ idx.name }}</div>
            <div class="idx-price">{{ idx.price.toFixed(2) }}</div>
            <div class="idx-change">
              {{ idx.change >= 0 ? "+" : "" }}{{ idx.change.toFixed(2) }}
              <span class="idx-pct">
                {{ idx.changePct >= 0 ? "+" : "" }}{{ idx.changePct.toFixed(2) }}%
              </span>
            </div>
            <div class="idx-ohlc">
              <span>开 {{ idx.open.toFixed(2) }}</span>
              <span>高 {{ idx.high.toFixed(2) }}</span>
              <span>低 {{ idx.low.toFixed(2) }}</span>
            </div>
            <div class="idx-vol">成交额 {{ formatTurnover(idx.turnover) }}</div>
          </div>
        </div>

        <!-- Watchlist -->
        <div class="watchlist-section">
          <h2 class="home-title">自选股</h2>
          <div v-if="watchlist.length === 0" class="home-hint" style="padding:16px 0">
            在左侧搜索股票，点击 ☆ 加入自选
          </div>
          <table v-else class="watchlist-table">
            <thead>
              <tr>
                <th>名称</th>
                <th>最新价</th>
                <th>涨跌</th>
                <th>涨跌幅</th>
                <th>状态</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="item in watchlist" :key="item.symbol">
                <td class="wl-name wl-name-link" @click="analyzeWatchlistStock(item.symbol)">
                  <span class="wl-stock-name">{{ item.name }}</span>
                  <span class="wl-code">{{ item.symbol }}</span>
                </td>
                <td :class="(watchlistQuotes.get(item.symbol)?.changePct ?? 0) >= 0 ? 'num-up' : 'num-down'">
                  {{ watchlistQuotes.get(item.symbol)?.price.toFixed(2) ?? '—' }}
                </td>
                <td :class="(watchlistQuotes.get(item.symbol)?.changePct ?? 0) >= 0 ? 'num-up' : 'num-down'">
                  {{ watchlistQuotes.has(item.symbol) ? ((watchlistQuotes.get(item.symbol)!.change >= 0 ? '+' : '') + watchlistQuotes.get(item.symbol)!.change.toFixed(2)) : '—' }}
                </td>
                <td :class="(watchlistQuotes.get(item.symbol)?.changePct ?? 0) >= 0 ? 'num-up' : 'num-down'">
                  {{ watchlistQuotes.has(item.symbol) ? ((watchlistQuotes.get(item.symbol)!.changePct >= 0 ? '+' : '') + watchlistQuotes.get(item.symbol)!.changePct.toFixed(2) + '%') : '—' }}
                </td>
                <td class="wl-status">
                  <span v-if="runningSymbols.has(item.symbol)" class="wl-running"><span class="spin">⟳</span> 分析中</span>
                  <template v-else-if="reportMetas.has(item.symbol)">
                    <span :class="['wl-decision', 'wl-' + reportMetas.get(item.symbol)!.decision]">
                      {{ reportMetas.get(item.symbol)!.decision }}
                    </span>
                    <span class="wl-time">{{ timeAgo(reportMetas.get(item.symbol)!.analyzedAt) }}</span>
                  </template>
                  <span v-else class="wl-time">未分析</span>
                </td>
                <td class="wl-actions">
                  <button
                    class="wl-btn wl-btn-view"
                    @click="reportMetas.has(item.symbol) ? viewSavedReport(item.symbol) : analyzeWatchlistStock(item.symbol)"
                  >{{ runningSymbols.has(item.symbol) ? '⟳ 查看' : '查看' }}</button>
                  <button class="wl-btn wl-btn-del" @click="removeFromWatchlist(item.symbol)">移除</button>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </div>

      <!-- Analysis view -->
      <div v-show="currentView === 'analysis'">
        <div class="analysis-back" @click="goHome">← 返回首页</div>

      <div v-for="[symbol, card] in cards" :key="symbol" v-show="analysisTarget && symbol === analysisTarget.symbol" class="stock-card">
        <!-- Card header -->
        <div class="card-header">
          <span class="stock-title">
            {{ card.name }}（{{ card.symbol }}）
            <span
              class="card-star"
              @click="isInWatchlist(card.symbol) ? removeFromWatchlist(card.symbol) : addToWatchlist(card.symbol, card.name)"
              :title="isInWatchlist(card.symbol) ? '取消关注' : '加入自选'"
            >{{ isInWatchlist(card.symbol) ? '★' : '☆' }}</span>
          </span>
          <span :class="['badge-decision', `badge-${card.decision}`]">
            {{ decisionLabel(card.decision) }}
          </span>
        </div>

        <!-- Chart -->
        <div :id="`chart-${symbol}`" class="chart-container"></div>

        <!-- Step timeline -->
        <div class="step-timeline">
          <div
            v-for="step in STEP_ORDER"
            :key="step"
            class="step-item"
          >
            <div
              :class="[
                'step-icon',
                card.steps[step]?.status || 'pending',
              ]"
            >
              <span v-if="card.steps[step]?.status === 'running'" class="spin"
                >⟳</span
              >
              <span v-else-if="card.steps[step]?.status === 'done'">✓</span>
              <span v-else>·</span>
            </div>
            <span>{{ STEP_LABELS[step]?.label || step }}</span>
          </div>
        </div>

        <!-- Report sections -->
        <div class="report-sections">
          <template v-for="step in STEP_ORDER" :key="step">
            <div
              v-if="
                step !== 'chart' &&
                card.steps[step]?.content
              "
              class="report-section"
            >
              <div
                class="section-title"
                @click="toggleSection(`${symbol}-${step}`)"
              >
                <span>{{ STEP_LABELS[step]?.label || step }}</span>
                <span>{{
                  expandedSections.has(`${symbol}-${step}`) ? "▲" : "▼"
                }}</span>
              </div>
              <div
                v-if="expandedSections.has(`${symbol}-${step}`)"
                class="section-body"
              >
                <div
                  v-if="!card.steps[step].streaming"
                  class="report-body"
                  v-html="renderMarkdown(card.steps[step].content)"
                ></div>
                <pre v-else class="stream-text">{{
                  stripThink(card.steps[step].content)
                }}</pre>
              </div>
            </div>
          </template>

          <!-- Final decision -->
          <div
            v-if="card.decision"
            :class="['final-decision', `final-${card.decision}`]"
          >
            <div
              :class="['decision-badge']"
              :style="{
                background:
                  card.decision === 'BUY'
                    ? '#ef5350'
                    : card.decision === 'SELL'
                      ? '#26a69a'
                      : '#ff9800',
                color: '#fff',
              }"
            >
              {{
                card.decision === "BUY"
                  ? "买入 (BUY)"
                  : card.decision === "SELL"
                    ? "卖出 (SELL)"
                    : "持有 (HOLD)"
              }}
            </div>
            <div class="decision-note">
              综合技术面、基本面、资讯面及多空辩论，给出以上投资建议。请结合自身风险偏好决策。
            </div>
          </div>
        </div>
      </div>
      </div>
    </div>

    <!-- Model Config Modal -->
    <div v-if="showConfigModal" class="modal-overlay" @click.self="showConfigModal = false">
      <div class="modal-dialog">
        <div class="modal-header">
          <h3>{{ configFormMode === 'list' ? '模型配置' : configFormMode === 'add' ? '添加模型' : '编辑模型' }}</h3>
          <span class="modal-close" @click="showConfigModal = false">✕</span>
        </div>

        <div v-if="configFormMode === 'list'" class="modal-body">
          <div v-if="modelConfigs.length === 0" class="config-empty">
            暂无模型配置，请添加
          </div>
          <div
            v-for="config in modelConfigs"
            :key="config.id"
            class="config-item"
            :class="{ 'config-active': config.id === activeModelId }"
            @click="setActiveModel(config.id)"
          >
            <div class="config-radio">
              <span v-if="config.id === activeModelId">●</span>
              <span v-else>○</span>
            </div>
            <div class="config-info-col">
              <div class="config-name">{{ config.name }}</div>
              <div class="config-detail">
                {{ providerIcon(config.provider) }} {{ config.provider }} · {{ config.model }}
                <span v-if="config.apiKey" class="config-key">· {{ maskApiKey(config.apiKey) }}</span>
              </div>
            </div>
            <div class="config-actions" @click.stop>
              <button class="config-btn" @click="startEditConfig(config)" title="编辑">✎</button>
              <button class="config-btn config-btn-del" @click="deleteModelConfig(config.id)" title="删除">✕</button>
            </div>
          </div>
          <button class="btn-add-config" @click="startAddConfig">+ 添加模型</button>
        </div>

        <div v-else class="modal-body">
          <div v-if="configError" class="config-error">{{ configError }}</div>
          <div class="form-group">
            <label>名称</label>
            <input v-model="configForm.name" placeholder="如：Claude Sonnet" class="form-input" />
          </div>
          <div class="form-group">
            <label>提供商</label>
            <select v-model="configForm.provider" class="form-input">
              <option value="anthropic">Anthropic (Claude)</option>
              <option value="openai">OpenAI</option>
              <option value="ollama">Ollama (本地)</option>
              <option value="minimax">MiniMax</option>
            </select>
          </div>
          <div class="form-group">
            <label>模型</label>
            <input
              v-model="configForm.model"
              :placeholder="configForm.provider === 'anthropic' ? 'claude-sonnet-4-6' :
                           configForm.provider === 'openai' ? 'gpt-4o' :
                           configForm.provider === 'ollama' ? 'hermes3' : 'MiniMax-M2.7-highspeed'"
              class="form-input"
            />
          </div>
          <div class="form-group">
            <label>API Key</label>
            <input
              v-model="configForm.apiKey"
              type="password"
              :placeholder="configForm.provider === 'ollama' ? '可选' : '必填'"
              class="form-input"
            />
          </div>
          <div class="form-group">
            <label>Base URL <small>（可选）</small></label>
            <input
              v-model="configForm.baseUrl"
              :placeholder="configForm.provider === 'ollama' ? 'http://localhost:11434/v1' :
                           configForm.provider === 'minimax' ? 'https://api.minimax.chat/v1' : '使用默认地址'"
              class="form-input"
            />
          </div>
          <div class="form-actions">
            <button class="btn-cancel" @click="cancelConfigForm">取消</button>
            <button class="btn-save" @click="saveConfig">保存</button>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style>
:root {
  --buy-color: #ef5350;
  --hold-color: #ff9800;
  --sell-color: #26a69a;
  --sidebar-w: 340px;
}

* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

html, body {
  height: 100%;
  overflow: hidden;
}

body {
  background: #f4f6f9;
  font-family: "PingFang SC", "Helvetica Neue", -apple-system, sans-serif;
  font-size: 14px;
  color: #333;
}

.app-container {
  display: flex;
  height: 100vh;
}

/* Sidebar */
.sidebar {
  width: var(--sidebar-w);
  min-width: var(--sidebar-w);
  background: #fff;
  border-right: 1px solid #e0e0e0;
  height: 100vh;
  overflow-y: auto;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.brand {
  font-weight: 700;
  font-size: 1.1rem;
  color: #1a237e;
}

.provider-badge {
  font-size: 0.78rem;
  color: #888;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 8px;
  border-radius: 6px;
  transition: background 0.15s;
}

.provider-badge:hover {
  background: #f0f4ff;
}

.config-gear {
  opacity: 0.4;
  font-size: 0.9rem;
}

.provider-badge:hover .config-gear {
  opacity: 0.8;
}

.nav-buttons {
  display: flex;
  gap: 6px;
}

.nav-btn {
  flex: 1;
  padding: 8px 12px;
  border: 1.5px solid #c5cae9;
  border-radius: 8px;
  background: #f8f9ff;
  cursor: pointer;
  font-size: 0.85rem;
  font-weight: 600;
  color: #1a237e;
  transition: all 0.15s;
}

.nav-btn:hover {
  background: #e8eaf6;
  border-color: #1a237e;
}

.section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.section-label {
  font-weight: 600;
  font-size: 0.85rem;
}

.search-wrapper {
  position: relative;
}

.search-input {
  width: 100%;
  padding: 6px 10px;
  border: 1px solid #ddd;
  border-radius: 6px;
  font-size: 0.88rem;
  outline: none;
}

.search-input:focus {
  border-color: #1a237e;
}

.search-dropdown {
  position: absolute;
  z-index: 1000;
  width: 100%;
  background: #fff;
  border: 1px solid #ddd;
  border-radius: 8px;
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.1);
  max-height: 260px;
  overflow-y: auto;
}

.dropdown-item {
  padding: 8px 14px;
  cursor: pointer;
  font-size: 0.9rem;
  display: flex;
  align-items: center;
}

.dropdown-item:hover {
  background: #f0f4ff;
}

.dropdown-item .code {
  color: #888;
  font-size: 0.8rem;
  margin-left: 6px;
}

.selected-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.stock-tag {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  background: #e8eaf6;
  color: #1a237e;
  border-radius: 20px;
  padding: 3px 10px 3px 12px;
  font-size: 0.85rem;
  font-weight: 500;
}

.stock-tag .remove {
  cursor: pointer;
  opacity: 0.6;
  margin-left: 4px;
}

.stock-tag .remove:hover {
  opacity: 1;
}

.date-inputs {
  display: flex;
  gap: 8px;
}

.date-input {
  flex: 1;
  padding: 6px 8px;
  border: 1px solid #ddd;
  border-radius: 6px;
  font-size: 0.85rem;
}

.range-buttons {
  display: flex;
  gap: 6px;
}

.range-buttons button {
  flex: 1;
  padding: 4px 8px;
  border: 1px solid #ccc;
  border-radius: 6px;
  background: #fff;
  cursor: pointer;
  font-size: 0.8rem;
}

.range-buttons button:hover {
  background: #f0f0f0;
}

.sidebar-footer {
  margin-top: auto;
}

.btn-primary {
  width: 100%;
  padding: 8px;
  background: #1a237e;
  color: #fff;
  border: none;
  border-radius: 8px;
  font-size: 0.9rem;
  cursor: pointer;
  font-weight: 600;
}

.btn-primary:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.btn-primary:hover:not(:disabled) {
  background: #283593;
}

.btn-danger {
  width: 100%;
  padding: 8px;
  background: #ef5350;
  color: #fff;
  border: none;
  border-radius: 8px;
  font-size: 0.9rem;
  cursor: pointer;
  font-weight: 600;
}

/* Main content */
.main-content {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 24px;
}

/* Home view */
.home-view {
  max-width: 900px;
  margin: 0 auto;
}

.home-title {
  font-size: 1.2rem;
  font-weight: 700;
  color: #1a237e;
  margin-bottom: 16px;
}

.index-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 16px;
  margin-bottom: 24px;
}

.index-card {
  background: #fff;
  border-radius: 12px;
  padding: 20px;
  box-shadow: 0 2px 12px rgba(0, 0, 0, 0.06);
  border-top: 3px solid #ccc;
}

.index-card.idx-up {
  border-top-color: var(--buy-color);
}

.index-card.idx-down {
  border-top-color: var(--sell-color);
}

.idx-name {
  font-size: 0.88rem;
  font-weight: 600;
  color: #555;
  margin-bottom: 8px;
}

.idx-price {
  font-size: 1.6rem;
  font-weight: 800;
  margin-bottom: 4px;
}

.idx-up .idx-price,
.idx-up .idx-change {
  color: var(--buy-color);
}

.idx-down .idx-price,
.idx-down .idx-change {
  color: var(--sell-color);
}

.idx-change {
  font-size: 0.92rem;
  font-weight: 600;
  margin-bottom: 10px;
}

.idx-pct {
  margin-left: 6px;
}

.idx-ohlc {
  display: flex;
  gap: 10px;
  font-size: 0.78rem;
  color: #888;
  margin-bottom: 4px;
}

.idx-vol {
  font-size: 0.78rem;
  color: #888;
}

.home-hint {
  text-align: center;
  padding: 32px 20px;
  color: #aaa;
  font-size: 0.92rem;
}

/* Watchlist */
.watchlist-section {
  margin-top: 8px;
}

.watchlist-table {
  width: 100%;
  border-collapse: collapse;
  background: #fff;
  border-radius: 10px;
  overflow: hidden;
  box-shadow: 0 2px 12px rgba(0, 0, 0, 0.06);
}

.watchlist-table th {
  background: #f5f6fa;
  padding: 10px 14px;
  font-size: 0.78rem;
  font-weight: 600;
  color: #888;
  text-align: left;
  white-space: nowrap;
}

.watchlist-table td {
  padding: 12px 14px;
  font-size: 0.88rem;
  border-top: 1px solid #f0f0f0;
  white-space: nowrap;
}

.watchlist-table tbody tr:hover {
  background: #f8f9ff;
}

.wl-name {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.wl-name-link {
  cursor: pointer;
}

.wl-name-link:hover .wl-stock-name {
  color: #1a237e;
  text-decoration: underline;
}

.wl-stock-name {
  font-weight: 600;
}

.wl-code {
  font-size: 0.75rem;
  color: #aaa;
}

.num-up {
  color: var(--buy-color);
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

.num-down {
  color: var(--sell-color);
  font-weight: 600;
  font-variant-numeric: tabular-nums;
}

.wl-meta {
  display: flex;
  align-items: center;
  gap: 6px;
}

.wl-decision {
  font-size: 0.72rem;
  font-weight: 700;
  padding: 2px 8px;
  border-radius: 4px;
}

.wl-BUY { background: #fff5f5; color: var(--buy-color); }
.wl-HOLD { background: #fffde7; color: var(--hold-color); }
.wl-SELL { background: #f0fff8; color: var(--sell-color); }

.wl-running {
  font-size: 0.8rem;
  color: #1976d2;
  font-weight: 600;
}

.wl-time {
  font-size: 0.75rem;
  color: #aaa;
}

.wl-actions {
  display: flex;
  gap: 6px;
}

.wl-btn {
  padding: 4px 12px;
  border: 1px solid #ddd;
  border-radius: 6px;
  background: #fff;
  cursor: pointer;
  font-size: 0.78rem;
  color: #555;
  white-space: nowrap;
}

.wl-btn:hover {
  background: #f0f4ff;
  border-color: #1a237e;
  color: #1a237e;
}

.wl-btn-view {
  color: #1a237e;
  border-color: #c5cae9;
}

.wl-btn-del:hover {
  background: #fff5f5;
  color: #ef5350;
  border-color: #ef5350;
}

/* Search dropdown star */
.watchlist-star {
  cursor: pointer;
  font-size: 1.1rem;
  color: #ccc;
  padding: 0 4px;
  flex-shrink: 0;
}

.watchlist-star:hover {
  color: #ff9800;
}

.watchlist-star.starred {
  color: #ff9800;
}

/* Analysis target */
.analysis-target {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 10px 12px;
  background: #e8eaf6;
  border-radius: 8px;
}

.target-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.target-name {
  font-weight: 700;
  font-size: 0.95rem;
  color: #1a237e;
}

.target-code {
  font-size: 0.78rem;
  color: #888;
}

/* Agent toggles */
.agent-toggles {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.agent-toggle {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 0.85rem;
  cursor: pointer;
  padding: 3px 0;
}

.agent-toggle input[type="checkbox"] {
  accent-color: #1a237e;
  width: 15px;
  height: 15px;
  cursor: pointer;
}

.analysis-back {
  display: inline-block;
  cursor: pointer;
  color: #1a237e;
  font-size: 0.9rem;
  font-weight: 500;
  margin-bottom: 16px;
  padding: 6px 12px;
  border-radius: 6px;
  transition: background 0.15s;
}

.analysis-back:hover {
  background: #e8eaf6;
}

/* Stock card */
.stock-card {
  background: #fff;
  border-radius: 12px;
  box-shadow: 0 2px 12px rgba(0, 0, 0, 0.06);
  margin-bottom: 28px;
  overflow: hidden;
}

.card-header {
  background: #1a237e;
  color: #fff;
  padding: 16px 20px;
  display: flex;
  align-items: center;
  justify-content: space-between;
}

.stock-title {
  font-size: 1.15rem;
  font-weight: 700;
  display: flex;
  align-items: center;
  gap: 8px;
}

.card-star {
  cursor: pointer;
  font-size: 1.1rem;
  opacity: 0.7;
  transition: opacity 0.15s;
}

.card-star:hover {
  opacity: 1;
}

.badge-decision {
  font-size: 0.85rem;
  font-weight: 600;
  border-radius: 20px;
  padding: 4px 14px;
  background: rgba(255, 255, 255, 0.2);
}

.badge-BUY {
  background: var(--buy-color) !important;
}
.badge-HOLD {
  background: var(--hold-color) !important;
}
.badge-SELL {
  background: var(--sell-color) !important;
}

.chart-container {
  width: 100%;
  min-height: 480px;
  overflow: hidden;
}

.chart-container .js-plotly-plot,
.chart-container .plot-container,
.chart-container .svg-container {
  width: 100% !important;
}

/* Step timeline */
.step-timeline {
  padding: 16px 20px;
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.step-item {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.85rem;
}

.step-icon {
  width: 24px;
  height: 24px;
  border-radius: 50%;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 0.7rem;
  flex-shrink: 0;
}

.step-icon.pending {
  background: #f0f0f0;
  color: #aaa;
}
.step-icon.running {
  background: #e3f2fd;
  color: #1976d2;
}
.step-icon.done {
  background: #e8f5e9;
  color: #388e3c;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}
.spin {
  animation: spin 1s linear infinite;
  display: inline-block;
}

/* Report sections */
.report-section {
  border-top: 1px solid #f0f0f0;
}

.section-title {
  padding: 12px 20px;
  cursor: pointer;
  user-select: none;
  display: flex;
  align-items: center;
  justify-content: space-between;
  font-weight: 600;
  font-size: 0.92rem;
  background: #fafafa;
}

.section-title:hover {
  background: #f0f4ff;
}

.section-body {
  padding: 16px 20px;
  font-size: 0.88rem;
  line-height: 1.7;
  border-top: 1px solid #f0f0f0;
}

.stream-text {
  white-space: pre-wrap;
  font-size: 0.82rem;
  line-height: 1.65;
  margin: 0;
  font-family: inherit;
  background: none;
  border: none;
  padding: 0;
}

/* Report body markdown */
.report-body table {
  width: 100%;
  border-collapse: collapse;
  margin: 12px 0;
}
.report-body th,
.report-body td {
  border: 1px solid #e0e0e0;
  padding: 7px 10px;
  font-size: 0.85rem;
}
.report-body th {
  background: #f5f5f5;
  font-weight: 600;
}
.report-body h1,
.report-body h2,
.report-body h3 {
  margin: 16px 0 8px;
}
.report-body p {
  margin: 8px 0;
}
.report-body ul,
.report-body ol {
  margin: 8px 0;
  padding-left: 24px;
}

/* Final decision */
.final-decision {
  border-radius: 10px;
  padding: 18px 20px;
  margin: 16px 20px;
}
.final-BUY {
  background: #fff5f5;
  border: 1.5px solid var(--buy-color);
}
.final-HOLD {
  background: #fffde7;
  border: 1.5px solid var(--hold-color);
}
.final-SELL {
  background: #f0fff8;
  border: 1.5px solid var(--sell-color);
}

.decision-badge {
  display: inline-block;
  font-size: 1.4rem;
  font-weight: 800;
  border-radius: 8px;
  padding: 6px 24px;
  margin-bottom: 12px;
}

.decision-note {
  font-size: 0.85rem;
  color: #555;
}

.error-banner {
  background: #fff5f5;
  border: 1.5px solid #ef5350;
  color: #c62828;
  padding: 16px 20px;
  border-radius: 10px;
  margin-bottom: 20px;
  cursor: pointer;
  font-size: 0.9rem;
  word-break: break-all;
}

/* Model Config Modal */
.modal-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(0, 0, 0, 0.4);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 2000;
}

.modal-dialog {
  background: #fff;
  border-radius: 12px;
  width: 460px;
  max-height: 80vh;
  overflow-y: auto;
  box-shadow: 0 8px 40px rgba(0, 0, 0, 0.15);
}

.modal-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 16px 20px;
  border-bottom: 1px solid #eee;
}

.modal-header h3 {
  font-size: 1rem;
  font-weight: 700;
  color: #1a237e;
}

.modal-close {
  cursor: pointer;
  font-size: 1.1rem;
  color: #999;
  padding: 4px;
}

.modal-close:hover {
  color: #333;
}

.modal-body {
  padding: 16px 20px;
}

.config-empty {
  text-align: center;
  padding: 24px;
  color: #aaa;
  font-size: 0.9rem;
}

.config-error {
  background: #fff5f5;
  border: 1px solid #ef5350;
  color: #c62828;
  padding: 8px 12px;
  border-radius: 6px;
  font-size: 0.85rem;
  margin-bottom: 12px;
}

.config-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px;
  border-radius: 8px;
  cursor: pointer;
  margin-bottom: 8px;
  border: 1.5px solid #eee;
  transition: all 0.15s;
}

.config-item:hover {
  border-color: #c5cae9;
  background: #f8f9ff;
}

.config-active {
  border-color: #1a237e !important;
  background: #e8eaf6 !important;
}

.config-radio {
  font-size: 1rem;
  color: #1a237e;
  flex-shrink: 0;
}

.config-info-col {
  flex: 1;
  min-width: 0;
}

.config-name {
  font-weight: 600;
  font-size: 0.9rem;
  margin-bottom: 2px;
}

.config-detail {
  font-size: 0.78rem;
  color: #888;
}

.config-key {
  color: #aaa;
}

.config-actions {
  display: flex;
  gap: 4px;
  flex-shrink: 0;
}

.config-btn {
  background: none;
  border: 1px solid #ddd;
  border-radius: 6px;
  padding: 4px 8px;
  cursor: pointer;
  font-size: 0.82rem;
  color: #666;
}

.config-btn:hover {
  background: #f0f0f0;
  color: #333;
}

.config-btn-del:hover {
  background: #fff5f5;
  color: #ef5350;
  border-color: #ef5350;
}

.btn-add-config {
  width: 100%;
  padding: 10px;
  border: 1.5px dashed #ccc;
  border-radius: 8px;
  background: none;
  cursor: pointer;
  font-size: 0.88rem;
  color: #888;
  margin-top: 4px;
}

.btn-add-config:hover {
  border-color: #1a237e;
  color: #1a237e;
  background: #f8f9ff;
}

.form-group {
  margin-bottom: 14px;
}

.form-group label {
  display: block;
  font-size: 0.82rem;
  font-weight: 600;
  margin-bottom: 4px;
  color: #555;
}

.form-group label small {
  font-weight: 400;
  color: #aaa;
}

.form-input {
  width: 100%;
  padding: 8px 10px;
  border: 1px solid #ddd;
  border-radius: 6px;
  font-size: 0.88rem;
  outline: none;
  font-family: inherit;
}

.form-input:focus {
  border-color: #1a237e;
}

select.form-input {
  appearance: auto;
}

.form-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 20px;
}

.btn-cancel {
  padding: 8px 20px;
  border: 1px solid #ddd;
  border-radius: 8px;
  background: #fff;
  cursor: pointer;
  font-size: 0.88rem;
}

.btn-cancel:hover {
  background: #f5f5f5;
}

.btn-save {
  padding: 8px 20px;
  border: none;
  border-radius: 8px;
  background: #1a237e;
  color: #fff;
  cursor: pointer;
  font-size: 0.88rem;
  font-weight: 600;
}

.btn-save:hover {
  background: #283593;
}
</style>
