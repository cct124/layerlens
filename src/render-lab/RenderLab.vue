<script setup lang="ts">
import { useRenderLab } from './useRenderLab';
const {
  setHost,
  setViewport,
  active,
  scene,
  assets,
  status,
  view,
  outline,
  hoverOutline,
  rebuild,
  zoom,
  reset,
  release,
  pointerDown,
  pointerMove,
  endPan,
  wheel,
  next,
  select,
  textMode,
  clearMetrics,
  viewportSize,
  largeExperiment,
  sourceLayers,
  fit,
  focusLayer,
} = useRenderLab();
</script>
<template>
  <main class="render-lab">
    <header>
      <div>
        <strong>LayerLens / Konva 渲染验证</strong>
        <p>独立开发实例 · 原稿只读 · Rust 场景</p>
      </div>
      <span>{{ active?.name ?? '等待 MCP 打开样本' }}</span>
    </header>
    <nav aria-label="渲染器">
      <button
        data-dev-target="render:rebuild"
        :disabled="status.phase === 'loading'"
        @click="rebuild"
      >
        重建 Konva 场景
      </button>
      <span class="phase">{{ status.phase }} · {{ status.backend || '未初始化' }}</span>
    </nav>
    <section class="tools">
      <template v-if="largeExperiment">
        <button
          data-dev-target="render:strict"
          :disabled="status.phase === 'loading'"
          @click="sourceLayers(false)"
        >
          严格场景
        </button>
        <button
          data-dev-target="render:source-layers"
          :disabled="status.phase === 'loading'"
          @click="sourceLayers(true)"
        >
          源图层诊断（非完整合成）
        </button>
      </template>
      <button
        data-dev-target="render:shared"
        :disabled="status.phase === 'loading'"
        :aria-pressed="status.textMode === 'shared'"
        @click="textMode('shared')"
      >
        共享布局
      </button>
      <button
        data-dev-target="render:native"
        :disabled="status.phase === 'loading'"
        :aria-pressed="status.textMode === 'native'"
        @click="textMode('native')"
      >
        原生排版
      </button>
      <button data-dev-target="render:clear-metrics" @click="clearMetrics">清空测量</button>
      <button
        data-dev-target="render:large"
        :disabled="status.phase === 'loading'"
        @click="viewportSize(true)"
      >
        960×600
      </button>
      <button
        data-dev-target="render:compact"
        :disabled="status.phase === 'loading'"
        @click="viewportSize(false)"
      >
        640×420
      </button>
    </section>
    <section class="tools">
      <button
        data-dev-target="render:auto"
        :aria-pressed="status.autoSelect"
        @click="status.autoSelect = !status.autoSelect"
      >
        自动选择 {{ status.autoSelect ? '开' : '关' }}
      </button>
      <span>Ctrl 临时选择 · 中键平移</span>
      <button data-dev-target="render:fit" :disabled="status.phase !== 'ready'" @click="fit">
        适应窗口
      </button>
      <button
        data-dev-target="render:focus"
        :disabled="status.phase !== 'ready' || active?.selectedLayerId == null"
        @click="focusLayer"
      >
        定位检查图层
      </button>
      <button
        v-for="n in [100, 200, 400]"
        :key="n"
        :data-dev-target="'render:' + n"
        :disabled="status.phase !== 'ready'"
        @click="zoom(n / 100)"
      >
        {{ n }}%
      </button>
      <button data-dev-target="render:reset" :disabled="status.phase !== 'ready'" @click="reset">
        复位
      </button>
      <button data-dev-target="render:next" :disabled="!status.candidateIds.length" @click="next">
        下一个重叠候选
      </button>
      <button data-dev-target="render:release" @click="release">释放场景</button>
    </section>
    <div v-if="status.error" role="alert" class="error">{{ status.error }}</div>
    <section class="workspace">
      <div>
        <div
          :ref="setViewport"
          class="viewport"
          tabindex="0"
          data-dev-target="canvas"
          :style="{ width: view.width + 'px', height: view.height + 'px' }"
          @pointerdown="pointerDown"
          @pointermove="pointerMove"
          @pointerup="endPan"
          @pointercancel="endPan"
          @lostpointercapture="endPan"
          @wheel="wheel"
          @contextmenu.prevent
        >
          <div :ref="setHost" class="engine-host"></div>
          <div v-if="outline" class="outline" :style="outline"></div>
          <div v-if="hoverOutline" class="outline hover-outline" :style="hoverOutline"></div>
          <div v-if="!scene" class="empty">通过开发测试 MCP 打开受控样本</div>
        </div>
        <p class="metrics">
          {{ Math.round(view.zoom * 100) }}% · DPR {{ view.dpr }} · 图元 {{ status.nodeCount }} ·
          显式持有对象 {{ status.resourceCount }} · 初始化 {{ status.initMs.toFixed(2) }} ms ·
          绘制就绪 {{ status.renderMs.toFixed(2) }} ms · 原生文字 {{ status.nativeTextLayers }} ·
          共享文字 {{ status.sharedTextLayers }} · 文字像素回退
          {{ status.textRasterLayerIds.length }} · 未绘制 {{ status.unsupportedLayers }} 层
        </p>
      </div>
      <aside>
        <h2>源文件合成参考</h2>
        <img v-if="assets" :src="assets.referenceUrl" alt="PSD 保存的合成图" />
        <p>参考来源以样本清单为准。生成样本的合成图不作为 Photoshop 保真参考。</p>
        <h2>图层与候选</h2>
        <button
          v-for="layer in scene?.layers.slice(0, largeExperiment ? 2048 : 128)"
          :key="layer.id"
          :data-dev-target="'layer:' + layer.id + ':select'"
          :class="{ selected: active?.selectedLayerId === layer.id, hidden: !layer.visible }"
          @click="select(layer.id)"
        >
          {{ layer.id }} · {{ layer.name }} <small>{{ layer.content.kind }}</small>
        </button>
      </aside>
    </section>
    <p v-if="!largeExperiment && scene && scene.layers.length > 128">
      列表显示前 128 层，渲染包含全部 {{ scene.layers.length }} 层。
    </p>
    <details>
      <summary>实验能力与证据限制</summary>
      <ul>
        <li v-for="note in status.limitations" :key="note">{{ note }}</li>
      </ul>
      <p>源 SHA-256：{{ status.sourceSha256 }}<br />字体 SHA-256：{{ status.fontSha256 }}</p>
    </details>
  </main>
</template>
