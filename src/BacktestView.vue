<script setup lang="ts">
import { ref, reactive, onMounted, nextTick, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import Plotly from "plotly.js-dist-min";

interface PresetParam {
  key: string;
  label: string;
  default: number;
  min: number;
  max: number;
  step: number;
}

interface PresetStrategy {
  id: string;
  name: string;
  description: string;
  params: PresetParam[];
  strategy: any;
}

interface BacktestMeta {
  id: string;
  symbol: string;
  name: string;
  strategyName: string;
  startDate: string;
  endDate: string;
  totalReturnPct: number;
  totalTrades: number;
  createdAt: string;
}

interface StockResult {
  code: string;
  name: string;
}

const emit = defineEmits<{
  (e: "go-home"): void;
}>();

// ── State ──
const mode = ref<"input" | "running" | "results">("input");
const strategyMode = ref<"preset" | "natural">("preset");

// Stock selection
const searchQuery = ref("");
const searchResults = ref<StockResult[]>([]);
const showDropdown = ref(false);
const selectedStock = ref<{ code: string; name: string } | null>(null);
let searchTimer: ReturnType<typeof setTimeout> | null = null;

// Date range
const startDate = ref("");
const endDate = ref("");
const initialCapital = ref(100000);

// Presets
const presets = ref<PresetStrategy[]>([]);
const selectedPresetId = ref("");
const presetParams = reactive<Record<string, number>>({});

// Natural language
const nlInput = ref("");
const translating = ref(false);
const translationError = ref("");

// Strategy result (from preset or NL translation)
const currentStrategy = ref<any>(null);
const strategyExplanation = ref("");

// Backtest results
const backtestRunning = ref(false);
const backtestError = ref("");
const currentResult = ref<any>(null);

// Progress
const progressStage = ref("");
const progressPercent = ref(0);
const progressBars = ref(0);

const stageLabels: Record<string, string> = {
  fetching: "正在获取行情数据…",
  computing: "正在计算指标与回测…",
  saving: "正在保存结果…",
  complete: "回测完成",
};

// History
const backtestHistory = ref<BacktestMeta[]>([]);

onMounted(async () => {
  setRange(365);
  await loadPresets();
  await loadHistory();
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

// ── Stock Search ──
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

function selectStock(code: string, name: string) {
  selectedStock.value = { code, name };
  searchQuery.value = `${name} (${code})`;
  showDropdown.value = false;
}

// ── Presets ──
async function loadPresets() {
  try {
    presets.value = await invoke("get_preset_strategies");
    if (presets.value.length > 0) {
      selectPreset(presets.value[0].id);
    }
  } catch {}
}

function selectPreset(id: string) {
  selectedPresetId.value = id;
  const preset = presets.value.find((p) => p.id === id);
  if (!preset) return;
  for (const p of preset.params) {
    presetParams[p.key] = p.default;
  }
  currentStrategy.value = applyPresetParams(preset);
  strategyExplanation.value = "";
}

function applyPresetParams(preset: PresetStrategy): any {
  const strategyJson = JSON.stringify(preset.strategy);
  let result = strategyJson;

  for (const p of preset.params) {
    const val = presetParams[p.key] ?? p.default;
    // Replace parameter values in the strategy JSON
    // This is a simple approach - params map to specific fields
    switch (p.key) {
      case "fast_period":
        result = result.replace(/"fast_period":\s*\d+/g, `"fast_period":${val}`);
        break;
      case "slow_period":
        result = result.replace(/"slow_period":\s*\d+/g, `"slow_period":${val}`);
        break;
      case "period":
        result = result.replace(/"period":\s*\d+/g, `"period":${val}`);
        break;
      case "oversold":
        // For RSI entries with threshold
        result = result.replace(
          /"indicator":\s*"rsi_crosses_below"[^}]*"threshold":\s*[\d.]+/,
          (m) => m.replace(/"threshold":\s*[\d.]+/, `"threshold":${val}`)
        );
        break;
      case "overbought":
        result = result.replace(
          /"indicator":\s*"rsi_crosses_above"[^}]*"threshold":\s*[\d.]+/,
          (m) => m.replace(/"threshold":\s*[\d.]+/, `"threshold":${val}`)
        );
        break;
      case "rsi_threshold":
        result = result.replace(
          /"indicator":\s*"rsi_below"[^}]*"threshold":\s*[\d.]+/,
          (m) => m.replace(/"threshold":\s*[\d.]+/, `"threshold":${val}`)
        );
        break;
      case "num_std":
        result = result.replace(/"num_std":\s*[\d.]+/g, `"num_std":${val}`);
        break;
      case "trailing_stop_pct":
        result = result.replace(
          /"trailing_stop":\s*\{[^}]*"percent":\s*[\d.]+/,
          (m) => m.replace(/"percent":\s*[\d.]+/, `"percent":${val}`)
        );
        break;
    }
  }
  return JSON.parse(result);
}

function onParamChange() {
  const preset = presets.value.find((p) => p.id === selectedPresetId.value);
  if (preset) {
    currentStrategy.value = applyPresetParams(preset);
  }
}

// ── Natural Language Translation ──
async function translateNL() {
  if (!nlInput.value.trim()) return;
  translating.value = true;
  translationError.value = "";
  strategyExplanation.value = "";

  try {
    const resultJson: string = await invoke("translate_strategy", {
      description: nlInput.value.trim(),
    });
    const result = JSON.parse(resultJson);
    currentStrategy.value = result.strategy;
    strategyExplanation.value = result.explanation || "";
  } catch (e: any) {
    translationError.value = String(e);
    currentStrategy.value = null;
  } finally {
    translating.value = false;
  }
}

// ── Run Backtest ──
async function runBacktest() {
  if (!selectedStock.value || !currentStrategy.value) return;

  backtestRunning.value = true;
  backtestError.value = "";
  progressStage.value = "";
  progressPercent.value = 0;
  progressBars.value = 0;
  mode.value = "running";

  let unlisten: UnlistenFn | null = null;
  try {
    unlisten = await listen<any>("backtest-progress", (event) => {
      const p = event.payload;
      progressStage.value = p.stage || "";
      progressPercent.value = p.percent || 0;
      if (p.bars) progressBars.value = p.bars;
    });

    const resultJson: string = await invoke("run_backtest", {
      symbol: selectedStock.value.code,
      name: selectedStock.value.name,
      startDate: startDate.value,
      endDate: endDate.value,
      strategyJson: JSON.stringify(currentStrategy.value),
      initialCapital: initialCapital.value,
      commissionRate: 0.0003,
      stampTaxRate: 0.001,
    });

    currentResult.value = toCamelCase(JSON.parse(resultJson));
    mode.value = "results";
    await loadHistory();
    await nextTick();
    renderEquityCurve();
  } catch (e: any) {
    backtestError.value = String(e);
    mode.value = "input";
  } finally {
    if (unlisten) unlisten();
    backtestRunning.value = false;
  }
}

// ── Results Rendering ──
function renderEquityCurve() {
  const el = document.getElementById("equity-chart");
  if (!el || !currentResult.value?.equityCurve) return;

  const curve = currentResult.value.equityCurve;
  const dates = curve.map((p: any) => p.date);
  const portfolio = curve.map((p: any) => p.portfolioValue);
  const benchmark = curve.map((p: any) => p.benchmarkValue);
  const drawdown = curve.map((p: any) => -p.drawdownPct);

  const trades = currentResult.value.trades || [];
  const entryDates = trades.map((t: any) => t.entryDate);
  const entryPrices = trades.map((t: any) => {
    const idx = curve.findIndex((p: any) => p.date === t.entryDate);
    return idx >= 0 ? curve[idx].portfolioValue : null;
  });
  const exitDates = trades.map((t: any) => t.exitDate);
  const exitPrices = trades.map((t: any) => {
    const idx = curve.findIndex((p: any) => p.date === t.exitDate);
    return idx >= 0 ? curve[idx].portfolioValue : null;
  });

  const data: any[] = [
    {
      x: dates,
      y: portfolio,
      type: "scatter",
      name: "策略净值",
      line: { color: "#1a237e", width: 2 },
      yaxis: "y",
    },
    {
      x: dates,
      y: benchmark,
      type: "scatter",
      name: "买入持有",
      line: { color: "#9e9e9e", width: 1.5, dash: "dash" },
      yaxis: "y",
    },
    {
      x: entryDates,
      y: entryPrices,
      type: "scatter",
      mode: "markers",
      name: "买入",
      marker: { symbol: "triangle-up", size: 10, color: "#ef5350" },
      yaxis: "y",
    },
    {
      x: exitDates,
      y: exitPrices,
      type: "scatter",
      mode: "markers",
      name: "卖出",
      marker: { symbol: "triangle-down", size: 10, color: "#26a69a" },
      yaxis: "y",
    },
    {
      x: dates,
      y: drawdown,
      type: "scatter",
      name: "回撤%",
      fill: "tozeroy",
      fillcolor: "rgba(239,83,80,0.1)",
      line: { color: "rgba(239,83,80,0.4)", width: 1 },
      yaxis: "y2",
    },
  ];

  const layout: any = {
    autosize: true,
    margin: { t: 30, b: 40, l: 60, r: 60 },
    showlegend: true,
    legend: { x: 0, y: 1.12, orientation: "h", font: { size: 11 } },
    xaxis: { type: "date" },
    yaxis: {
      title: "净值",
      side: "left",
      showgrid: true,
      gridcolor: "#f0f0f0",
    },
    yaxis2: {
      title: "回撤%",
      side: "right",
      overlaying: "y",
      showgrid: false,
      range: [-50, 0],
    },
    plot_bgcolor: "#fff",
    paper_bgcolor: "#fff",
  };

  Plotly.newPlot(el, data, layout, { responsive: true, displayModeBar: false });
}

watch(
  () => mode.value,
  async (v) => {
    if (v === "results") {
      await nextTick();
      renderEquityCurve();
    }
  }
);

// ── History ──
async function loadHistory() {
  try {
    backtestHistory.value = await invoke("list_backtests");
  } catch {}
}

async function loadHistoryResult(id: string) {
  try {
    const record: any = await invoke("get_backtest", { id });
    if (!record) return;
    currentResult.value = toCamelCase(JSON.parse(record.resultJson));
    currentResult.value.id = record.id;
    selectedStock.value = { code: record.symbol, name: record.name };
    searchQuery.value = `${record.name} (${record.symbol})`;
    mode.value = "results";
    await nextTick();
    renderEquityCurve();
  } catch (e) {
    console.error("Failed to load backtest record:", e);
    backtestError.value = "加载历史记录失败";
  }
}

async function deleteHistory(id: string) {
  try {
    await invoke("delete_backtest", { id });
    await loadHistory();
    if (currentResult.value?.id === id) {
      currentResult.value = null;
      mode.value = "input";
    }
  } catch (e) {
    console.error("Failed to delete backtest:", e);
    backtestError.value = "删除失败";
  }
}

function formatPct(val: number): string {
  const sign = val >= 0 ? "+" : "";
  return `${sign}${val.toFixed(2)}%`;
}

function formatNum(val: number): string {
  return val.toLocaleString("zh-CN", { maximumFractionDigits: 2 });
}

function timeAgo(isoStr: string): string {
  const d = new Date(isoStr);
  const diff = Date.now() - d.getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return "刚刚";
  if (mins < 60) return `${mins}分钟前`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}小时前`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}天前`;
  return d.toLocaleDateString();
}

function toCamelCase(obj: any): any {
  if (Array.isArray(obj)) return obj.map(toCamelCase);
  if (obj && typeof obj === "object") {
    return Object.fromEntries(
      Object.entries(obj).map(([k, v]) => [
        k.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase()),
        toCamelCase(v),
      ])
    );
  }
  return obj;
}

function exitReasonLabel(reason: string): string {
  const labels: Record<string, string> = {
    signal: "信号",
    stop_loss: "止损",
    take_profit: "止盈",
    trailing_stop: "移动止损",
    end_of_period: "到期",
  };
  return labels[reason] || reason;
}

function backToInput() {
  mode.value = "input";
  currentResult.value = null;
}

function describeIndicator(ic: any): string {
  const labels: Record<string, (p: any) => string> = {
    rsi_above: (p) => `RSI(${p.period}) > ${p.threshold}`,
    rsi_below: (p) => `RSI(${p.period}) < ${p.threshold}`,
    rsi_crosses_above: (p) => `RSI(${p.period}) 上穿 ${p.threshold}`,
    rsi_crosses_below: (p) => `RSI(${p.period}) 下穿 ${p.threshold}`,
    sma_crosses_above_sma: (p) => `SMA(${p.fast_period}) 金叉 SMA(${p.slow_period})`,
    sma_crosses_below_sma: (p) => `SMA(${p.fast_period}) 死叉 SMA(${p.slow_period})`,
    ema_crosses_above_ema: (p) => `EMA(${p.fast_period}) 金叉 EMA(${p.slow_period})`,
    ema_crosses_below_ema: (p) => `EMA(${p.fast_period}) 死叉 EMA(${p.slow_period})`,
    price_above_sma: (p) => `价格 > SMA(${p.period})`,
    price_below_sma: (p) => `价格 < SMA(${p.period})`,
    price_above_ema: (p) => `价格 > EMA(${p.period})`,
    price_below_ema: (p) => `价格 < EMA(${p.period})`,
    macd_crosses_above_signal: () => "MACD 金叉",
    macd_crosses_below_signal: () => "MACD 死叉",
    macd_histogram_positive: () => "MACD柱 > 0",
    macd_histogram_negative: () => "MACD柱 < 0",
    price_below_lower_boll: (p) => `价格 < 布林下轨(${p.period}, ${p.num_std}σ)`,
    price_above_upper_boll: (p) => `价格 > 布林上轨(${p.period}, ${p.num_std}σ)`,
    price_crosses_above_lower_boll: (p) => `价格上穿布林下轨(${p.period})`,
    price_crosses_below_upper_boll: (p) => `价格下穿布林上轨(${p.period})`,
    price_above: (p) => `价格 > ${p.price}`,
    price_below: (p) => `价格 < ${p.price}`,
    volume_above_avg: (p) => `成交量 > ${p.multiplier}x均量(${p.period})`,
  };
  const fn = labels[ic.indicator];
  return fn ? fn(ic.params || {}) : ic.indicator;
}

function describeGroup(group: any): string {
  if (!group?.conditions?.length) return "无";
  const parts = group.conditions.map((c: any) => {
    if (c.type === "indicator") return describeIndicator(c);
    if (c.type === "group") return `(${describeGroup(c)})`;
    return "?";
  });
  const joiner = group.logic === "or" ? " 或 " : " 且 ";
  return parts.join(joiner);
}

function describeStop(stop: any): string {
  if (stop.type === "percentage") return `${stop.percent}%`;
  if (stop.type === "fixed_price") return `¥${stop.price}`;
  return JSON.stringify(stop);
}
</script>

<template>
  <div class="bt-container">
    <!-- Sidebar -->
    <div class="bt-sidebar">
      <div class="bt-back" @click="emit('go-home')">← 返回首页</div>
      <div class="bt-brand">回测系统</div>

      <!-- Stock selector -->
      <div class="bt-section">
        <label class="bt-label">选择股票</label>
        <div class="bt-search-wrapper">
          <input
            v-model="searchQuery"
            @input="onSearchInput"
            @focus="() => { if (searchQuery && !selectedStock) doSearch(); }"
            placeholder="输入代码或名称…"
            class="bt-input"
          />
          <div v-if="showDropdown" class="bt-dropdown" @mousedown.prevent>
            <div
              v-for="s in searchResults"
              :key="s.code"
              class="bt-dropdown-item"
              @click="selectStock(s.code, s.name)"
            >
              {{ s.name }}
              <span class="bt-code">{{ s.code }}</span>
            </div>
          </div>
        </div>
      </div>

      <!-- Date range -->
      <div class="bt-section">
        <label class="bt-label">回测区间</label>
        <div class="bt-dates">
          <input v-model="startDate" type="date" class="bt-input bt-date" />
          <input v-model="endDate" type="date" class="bt-input bt-date" />
        </div>
        <div class="bt-range-btns">
          <button @click="setRange(180)">6个月</button>
          <button @click="setRange(365)">1年</button>
          <button @click="setRange(730)">2年</button>
          <button @click="setRange(1095)">3年</button>
        </div>
      </div>

      <!-- Initial capital -->
      <div class="bt-section">
        <label class="bt-label">初始资金</label>
        <input
          v-model.number="initialCapital"
          type="number"
          step="10000"
          min="10000"
          class="bt-input"
        />
      </div>

      <!-- Run button -->
      <div class="bt-sidebar-footer">
        <button
          class="bt-btn-run"
          @click="runBacktest"
          :disabled="!selectedStock || !currentStrategy || backtestRunning"
        >
          {{ backtestRunning ? '⟳ 回测中…' : '▶ 开始回测' }}
        </button>
        <button v-if="mode === 'results'" class="bt-btn-back" @click="backToInput">
          修改策略
        </button>
      </div>

      <!-- History -->
      <div class="bt-section bt-history-section" v-if="backtestHistory.length > 0">
        <label class="bt-label">历史记录</label>
        <div class="bt-history-list">
          <div
            v-for="h in backtestHistory"
            :key="h.id"
            class="bt-history-item"
            :class="{ active: currentResult?.id === h.id }"
            @click="loadHistoryResult(h.id)"
          >
            <div class="bt-hist-main">
              <span class="bt-hist-name">{{ h.name }}</span>
              <span class="bt-hist-strategy">{{ h.strategyName }}</span>
            </div>
            <div class="bt-hist-meta">
              <span :class="h.totalReturnPct >= 0 ? 'num-up' : 'num-down'">
                {{ formatPct(h.totalReturnPct) }}
              </span>
              <span class="bt-hist-time">{{ timeAgo(h.createdAt) }}</span>
            </div>
            <button class="bt-hist-del" @click.stop="deleteHistory(h.id)" title="删除">✕</button>
          </div>
        </div>
      </div>
    </div>

    <!-- Main content -->
    <div class="bt-main">
      <!-- Input mode -->
      <div v-if="mode === 'input'" class="bt-input-panel">
        <!-- Strategy mode tabs -->
        <div class="bt-tabs">
          <button
            :class="{ active: strategyMode === 'preset' }"
            @click="strategyMode = 'preset'"
          >
            预设策略
          </button>
          <button
            :class="{ active: strategyMode === 'natural' }"
            @click="strategyMode = 'natural'"
          >
            自然语言
          </button>
        </div>

        <!-- Preset mode -->
        <div v-if="strategyMode === 'preset'" class="bt-preset-panel">
          <div class="bt-preset-grid">
            <div
              v-for="p in presets"
              :key="p.id"
              class="bt-preset-card"
              :class="{ selected: selectedPresetId === p.id }"
              @click="selectPreset(p.id)"
            >
              <div class="bt-preset-name">{{ p.name }}</div>
              <div class="bt-preset-desc">{{ p.description }}</div>
            </div>
          </div>

          <!-- Preset params -->
          <div
            v-if="selectedPresetId"
            class="bt-params"
          >
            <template v-for="p in presets.find(x => x.id === selectedPresetId)?.params" :key="p.key">
              <div class="bt-param-row">
                <label>{{ p.label }}</label>
                <div class="bt-param-control">
                  <input
                    type="range"
                    :min="p.min"
                    :max="p.max"
                    :step="p.step"
                    v-model.number="presetParams[p.key]"
                    @input="onParamChange"
                    class="bt-slider"
                  />
                  <input
                    type="number"
                    :min="p.min"
                    :max="p.max"
                    :step="p.step"
                    v-model.number="presetParams[p.key]"
                    @change="onParamChange"
                    class="bt-param-num"
                  />
                </div>
              </div>
            </template>
          </div>
        </div>

        <!-- Natural language mode -->
        <div v-if="strategyMode === 'natural'" class="bt-nl-panel">
          <textarea
            v-model="nlInput"
            placeholder="用自然语言描述你的交易策略…&#10;&#10;例如：&#10;- 当RSI低于30且价格在60日均线上方时买入，RSI超过70时卖出&#10;- 20日均线上穿50日均线金叉买入，死叉卖出，设置8%止损&#10;- MACD金叉且成交量放大1.5倍时买入，MACD死叉卖出，5%移动止损"
            class="bt-textarea"
            rows="6"
          ></textarea>
          <button
            class="bt-btn-translate"
            @click="translateNL"
            :disabled="!nlInput.trim() || translating"
          >
            {{ translating ? '⟳ 翻译中…' : '翻译为策略' }}
          </button>
          <div v-if="translationError" class="bt-error">{{ translationError }}</div>
          <div v-if="strategyExplanation" class="bt-explanation">{{ strategyExplanation }}</div>
        </div>

        <!-- Strategy preview -->
        <div v-if="currentStrategy" class="bt-strategy-preview">
          <div class="bt-preview-header">
            <span class="bt-preview-title">{{ currentStrategy.name }}</span>
            <span class="bt-preview-desc">{{ currentStrategy.description }}</span>
          </div>
          <div class="bt-preview-details">
            <div class="bt-preview-row">
              <span class="bt-preview-label">入场条件</span>
              <span class="bt-preview-value">{{ describeGroup(currentStrategy.entry) }}</span>
            </div>
            <div class="bt-preview-row">
              <span class="bt-preview-label">出场条件</span>
              <span class="bt-preview-value">{{ describeGroup(currentStrategy.exit) }}</span>
            </div>
            <div v-if="currentStrategy.stop_loss" class="bt-preview-row">
              <span class="bt-preview-label">止损</span>
              <span class="bt-preview-value">{{ describeStop(currentStrategy.stop_loss) }}</span>
            </div>
            <div v-if="currentStrategy.take_profit" class="bt-preview-row">
              <span class="bt-preview-label">止盈</span>
              <span class="bt-preview-value">{{ describeStop(currentStrategy.take_profit) }}</span>
            </div>
            <div v-if="currentStrategy.trailing_stop" class="bt-preview-row">
              <span class="bt-preview-label">移动止损</span>
              <span class="bt-preview-value">{{ currentStrategy.trailing_stop.percent }}%</span>
            </div>
          </div>
        </div>
      </div>

      <!-- Running mode -->
      <div v-if="mode === 'running'" class="bt-running">
        <div class="bt-spinner">⟳</div>
        <div class="bt-running-text">
          {{ stageLabels[progressStage] || '正在准备…' }}
        </div>
        <div class="bt-progress-bar">
          <div class="bt-progress-fill" :style="{ width: progressPercent + '%' }"></div>
        </div>
        <div class="bt-progress-detail">
          <span v-if="progressBars > 0">{{ progressBars }} 根K线</span>
          <span>{{ progressPercent }}%</span>
        </div>
      </div>

      <!-- Results mode -->
      <div v-if="mode === 'results' && currentResult" class="bt-results">
        <!-- Metrics cards -->
        <div class="bt-metrics-grid">
          <div class="bt-metric-card" :class="currentResult.metrics.totalReturnPct >= 0 ? 'metric-up' : 'metric-down'">
            <div class="bt-metric-label">总收益</div>
            <div class="bt-metric-value">{{ formatPct(currentResult.metrics.totalReturnPct) }}</div>
          </div>
          <div class="bt-metric-card" :class="currentResult.metrics.annualizedReturnPct >= 0 ? 'metric-up' : 'metric-down'">
            <div class="bt-metric-label">年化收益</div>
            <div class="bt-metric-value">{{ formatPct(currentResult.metrics.annualizedReturnPct) }}</div>
          </div>
          <div class="bt-metric-card">
            <div class="bt-metric-label">夏普比率</div>
            <div class="bt-metric-value">{{ currentResult.metrics.sharpeRatio.toFixed(2) }}</div>
          </div>
          <div class="bt-metric-card metric-down">
            <div class="bt-metric-label">最大回撤</div>
            <div class="bt-metric-value">-{{ currentResult.metrics.maxDrawdownPct.toFixed(2) }}%</div>
          </div>
          <div class="bt-metric-card">
            <div class="bt-metric-label">胜率</div>
            <div class="bt-metric-value">{{ (currentResult.metrics.winRate * 100).toFixed(1) }}%</div>
          </div>
          <div class="bt-metric-card">
            <div class="bt-metric-label">盈亏比</div>
            <div class="bt-metric-value">{{ currentResult.metrics.profitFactor >= 9999 ? '∞' : currentResult.metrics.profitFactor?.toFixed(2) ?? '-' }}</div>
          </div>
          <div class="bt-metric-card">
            <div class="bt-metric-label">总交易次数</div>
            <div class="bt-metric-value">{{ currentResult.metrics.totalTrades }}</div>
          </div>
          <div class="bt-metric-card" :class="currentResult.metrics.benchmarkReturnPct >= 0 ? 'metric-up' : 'metric-down'">
            <div class="bt-metric-label">基准收益</div>
            <div class="bt-metric-value">{{ formatPct(currentResult.metrics.benchmarkReturnPct) }}</div>
          </div>
        </div>

        <!-- Equity curve chart -->
        <div class="bt-chart-section">
          <h3>净值曲线</h3>
          <div id="equity-chart" class="bt-chart"></div>
        </div>

        <!-- Trade log -->
        <div class="bt-trades-section">
          <h3>交易记录 ({{ currentResult.trades.length }})</h3>
          <div class="bt-trades-scroll">
            <table class="bt-trades-table" v-if="currentResult.trades.length > 0">
              <thead>
                <tr>
                  <th>买入日期</th>
                  <th>买入价</th>
                  <th>卖出日期</th>
                  <th>卖出价</th>
                  <th>股数</th>
                  <th>盈亏</th>
                  <th>收益率</th>
                  <th>持仓天数</th>
                  <th>退出原因</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="(t, i) in currentResult.trades" :key="i">
                  <td>{{ t.entryDate }}</td>
                  <td>{{ t.entryPrice.toFixed(2) }}</td>
                  <td>{{ t.exitDate }}</td>
                  <td>{{ t.exitPrice.toFixed(2) }}</td>
                  <td>{{ t.shares }}</td>
                  <td :class="t.pnl >= 0 ? 'num-up' : 'num-down'">{{ formatNum(t.pnl) }}</td>
                  <td :class="t.pnlPct >= 0 ? 'num-up' : 'num-down'">{{ formatPct(t.pnlPct) }}</td>
                  <td>{{ t.holdingDays }}</td>
                  <td>{{ exitReasonLabel(t.exitReason) }}</td>
                </tr>
              </tbody>
            </table>
            <div v-else class="bt-no-trades">无交易记录</div>
          </div>
        </div>
      </div>

      <!-- Error display -->
      <div v-if="backtestError" class="bt-error-banner" @click="backtestError = ''">
        {{ backtestError }}
        <small>点击关闭</small>
      </div>
    </div>
  </div>
</template>

<style scoped>
.bt-container {
  display: flex;
  height: 100vh;
}

.bt-sidebar {
  width: 300px;
  min-width: 300px;
  background: #fff;
  border-right: 1px solid #e0e0e0;
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 14px;
  overflow-y: auto;
}

.bt-back {
  cursor: pointer;
  color: #1a237e;
  font-size: 0.88rem;
  font-weight: 500;
  padding: 4px 0;
}

.bt-back:hover {
  text-decoration: underline;
}

.bt-brand {
  font-weight: 700;
  font-size: 1.05rem;
  color: #1a237e;
}

.bt-section {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.bt-label {
  font-weight: 600;
  font-size: 0.82rem;
  color: #555;
}

.bt-input {
  width: 100%;
  padding: 6px 10px;
  border: 1px solid #ddd;
  border-radius: 6px;
  font-size: 0.85rem;
  outline: none;
  font-family: inherit;
}

.bt-input:focus {
  border-color: #1a237e;
}

.bt-search-wrapper {
  position: relative;
}

.bt-dropdown {
  position: absolute;
  z-index: 1000;
  width: 100%;
  background: #fff;
  border: 1px solid #ddd;
  border-radius: 8px;
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.1);
  max-height: 220px;
  overflow-y: auto;
}

.bt-dropdown-item {
  padding: 8px 12px;
  cursor: pointer;
  font-size: 0.88rem;
}

.bt-dropdown-item:hover {
  background: #f0f4ff;
}

.bt-code {
  color: #888;
  font-size: 0.78rem;
  margin-left: 6px;
}

.bt-dates {
  display: flex;
  gap: 6px;
}

.bt-date {
  flex: 1;
}

.bt-range-btns {
  display: flex;
  gap: 4px;
}

.bt-range-btns button {
  flex: 1;
  padding: 4px 6px;
  border: 1px solid #ccc;
  border-radius: 6px;
  background: #fff;
  cursor: pointer;
  font-size: 0.75rem;
}

.bt-range-btns button:hover {
  background: #f0f0f0;
}

.bt-sidebar-footer {
  margin-top: auto;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.bt-btn-run {
  width: 100%;
  padding: 8px;
  background: #1a237e;
  color: #fff;
  border: none;
  border-radius: 8px;
  font-size: 0.88rem;
  font-weight: 600;
  cursor: pointer;
}

.bt-btn-run:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.bt-btn-run:hover:not(:disabled) {
  background: #283593;
}

.bt-btn-back {
  width: 100%;
  padding: 6px;
  background: none;
  border: 1px solid #ddd;
  border-radius: 8px;
  font-size: 0.82rem;
  cursor: pointer;
  color: #666;
}

.bt-btn-back:hover {
  background: #f5f5f5;
}

/* History */
.bt-history-section {
  border-top: 1px solid #eee;
  padding-top: 12px;
}

.bt-history-list {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 300px;
  overflow-y: auto;
}

.bt-history-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  border-radius: 8px;
  cursor: pointer;
  border: 1px solid #eee;
  font-size: 0.82rem;
}

.bt-history-item:hover {
  background: #f8f9ff;
  border-color: #c5cae9;
}

.bt-history-item.active {
  background: #e8eaf6;
  border-color: #1a237e;
}

.bt-hist-main {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.bt-hist-name {
  font-weight: 600;
  font-size: 0.8rem;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.bt-hist-strategy {
  font-size: 0.72rem;
  color: #888;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.bt-hist-meta {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 2px;
  flex-shrink: 0;
}

.bt-hist-time {
  font-size: 0.68rem;
  color: #aaa;
}

.bt-hist-del {
  background: none;
  border: none;
  cursor: pointer;
  color: #ccc;
  font-size: 0.82rem;
  padding: 2px;
  flex-shrink: 0;
}

.bt-hist-del:hover {
  color: #ef5350;
}

/* Main content */
.bt-main {
  flex: 1;
  overflow-y: auto;
  padding: 24px;
}

/* Tabs */
.bt-tabs {
  display: flex;
  gap: 0;
  margin-bottom: 20px;
  border-bottom: 2px solid #eee;
}

.bt-tabs button {
  padding: 10px 24px;
  border: none;
  background: none;
  cursor: pointer;
  font-size: 0.92rem;
  font-weight: 500;
  color: #888;
  border-bottom: 2px solid transparent;
  margin-bottom: -2px;
  transition: all 0.15s;
}

.bt-tabs button.active {
  color: #1a237e;
  border-bottom-color: #1a237e;
  font-weight: 700;
}

.bt-tabs button:hover {
  color: #333;
}

/* Preset panel */
.bt-preset-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 12px;
  margin-bottom: 20px;
}

.bt-preset-card {
  padding: 14px;
  border: 1.5px solid #eee;
  border-radius: 10px;
  cursor: pointer;
  transition: all 0.15s;
}

.bt-preset-card:hover {
  border-color: #c5cae9;
  background: #f8f9ff;
}

.bt-preset-card.selected {
  border-color: #1a237e;
  background: #e8eaf6;
}

.bt-preset-name {
  font-weight: 700;
  font-size: 0.92rem;
  margin-bottom: 4px;
}

.bt-preset-desc {
  font-size: 0.78rem;
  color: #888;
  line-height: 1.4;
}

/* Params */
.bt-params {
  background: #fafafa;
  border-radius: 10px;
  padding: 16px;
  margin-bottom: 20px;
}

.bt-param-row {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-bottom: 10px;
}

.bt-param-row:last-child {
  margin-bottom: 0;
}

.bt-param-row label {
  width: 120px;
  font-size: 0.82rem;
  font-weight: 500;
  flex-shrink: 0;
}

.bt-param-control {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 10px;
}

.bt-slider {
  flex: 1;
  accent-color: #1a237e;
}

.bt-param-num {
  width: 70px;
  padding: 4px 8px;
  border: 1px solid #ddd;
  border-radius: 6px;
  font-size: 0.82rem;
  text-align: center;
}

/* Natural language */
.bt-nl-panel {
  margin-bottom: 20px;
}

.bt-textarea {
  width: 100%;
  padding: 12px;
  border: 1px solid #ddd;
  border-radius: 8px;
  font-size: 0.88rem;
  font-family: inherit;
  resize: vertical;
  outline: none;
  line-height: 1.6;
}

.bt-textarea:focus {
  border-color: #1a237e;
}

.bt-btn-translate {
  margin-top: 10px;
  padding: 8px 20px;
  background: #1a237e;
  color: #fff;
  border: none;
  border-radius: 8px;
  font-size: 0.85rem;
  font-weight: 600;
  cursor: pointer;
}

.bt-btn-translate:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.bt-error {
  margin-top: 10px;
  padding: 10px 14px;
  background: #fff5f5;
  border: 1px solid #ef5350;
  color: #c62828;
  border-radius: 8px;
  font-size: 0.85rem;
}

.bt-explanation {
  margin-top: 10px;
  padding: 10px 14px;
  background: #e8f5e9;
  border-radius: 8px;
  font-size: 0.85rem;
  color: #2e7d32;
}

/* Strategy preview */
.bt-strategy-preview {
  background: #fff;
  border: 1px solid #e0e0e0;
  border-radius: 10px;
  overflow: hidden;
}

.bt-preview-header {
  background: #1a237e;
  color: #fff;
  padding: 12px 16px;
}

.bt-preview-title {
  font-weight: 700;
  font-size: 0.95rem;
  display: block;
  margin-bottom: 4px;
}

.bt-preview-desc {
  font-size: 0.78rem;
  opacity: 0.8;
}

.bt-preview-details {
  padding: 14px 16px;
}

.bt-preview-row {
  display: flex;
  gap: 10px;
  padding: 6px 0;
  border-bottom: 1px solid #f5f5f5;
  font-size: 0.85rem;
}

.bt-preview-row:last-child {
  border-bottom: none;
}

.bt-preview-label {
  width: 80px;
  font-weight: 600;
  color: #555;
  flex-shrink: 0;
}

.bt-preview-value {
  color: #333;
}

/* Running */
.bt-running {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 300px;
  gap: 16px;
}

.bt-spinner {
  font-size: 3rem;
  animation: spin 1s linear infinite;
  display: inline-block;
  color: #1a237e;
}

.bt-running-text {
  font-size: 1rem;
  color: #555;
  font-weight: 500;
}

.bt-progress-bar {
  width: 320px;
  height: 6px;
  background: #e0e0e0;
  border-radius: 3px;
  overflow: hidden;
}

.bt-progress-fill {
  height: 100%;
  background: #1a237e;
  border-radius: 3px;
  transition: width 0.3s ease;
}

.bt-progress-detail {
  display: flex;
  gap: 12px;
  font-size: 0.78rem;
  color: #aaa;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

/* Results */
.bt-metrics-grid {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 12px;
  margin-bottom: 24px;
}

.bt-metric-card {
  background: #fff;
  border-radius: 10px;
  padding: 16px;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.05);
  border-top: 3px solid #e0e0e0;
}

.bt-metric-card.metric-up {
  border-top-color: #ef5350;
}

.bt-metric-card.metric-down {
  border-top-color: #26a69a;
}

.bt-metric-label {
  font-size: 0.78rem;
  color: #888;
  margin-bottom: 6px;
}

.bt-metric-value {
  font-size: 1.3rem;
  font-weight: 800;
}

.metric-up .bt-metric-value {
  color: #ef5350;
}

.metric-down .bt-metric-value {
  color: #26a69a;
}

.bt-chart-section {
  background: #fff;
  border-radius: 10px;
  padding: 20px;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.05);
  margin-bottom: 24px;
}

.bt-chart-section h3 {
  font-size: 0.95rem;
  font-weight: 700;
  color: #1a237e;
  margin-bottom: 12px;
}

.bt-chart {
  width: 100%;
  min-height: 400px;
}

.bt-trades-section {
  background: #fff;
  border-radius: 10px;
  padding: 20px;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.05);
}

.bt-trades-section h3 {
  font-size: 0.95rem;
  font-weight: 700;
  color: #1a237e;
  margin-bottom: 12px;
}

.bt-trades-scroll {
  overflow-x: auto;
}

.bt-trades-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.82rem;
}

.bt-trades-table th {
  background: #f5f6fa;
  padding: 8px 10px;
  font-weight: 600;
  color: #888;
  text-align: left;
  white-space: nowrap;
  font-size: 0.78rem;
}

.bt-trades-table td {
  padding: 8px 10px;
  border-top: 1px solid #f0f0f0;
  white-space: nowrap;
}

.bt-trades-table tbody tr:hover {
  background: #f8f9ff;
}

.num-up {
  color: #ef5350;
  font-weight: 600;
}

.num-down {
  color: #26a69a;
  font-weight: 600;
}

.bt-no-trades {
  text-align: center;
  padding: 24px;
  color: #aaa;
}

.bt-error-banner {
  position: fixed;
  bottom: 20px;
  right: 20px;
  background: #fff5f5;
  border: 1.5px solid #ef5350;
  color: #c62828;
  padding: 14px 20px;
  border-radius: 10px;
  cursor: pointer;
  font-size: 0.88rem;
  max-width: 400px;
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.1);
  z-index: 3000;
}

.bt-error-banner small {
  display: block;
  margin-top: 4px;
  opacity: 0.7;
}
</style>
