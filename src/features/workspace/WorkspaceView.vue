<script setup lang="ts">
import { useAppStatus } from './useAppStatus';

const { status, checkStatus } = useAppStatus();
</script>

<template>
  <div class="app-shell">
    <header class="app-header">
      <div class="brand">
        <svg class="brand-mark" viewBox="0 0 32 32" fill="none" aria-hidden="true">
          <path d="m16 4 12 7-12 7-12-7L16 4Z" fill="currentColor" />
          <path d="m4 17 12 7 12-7M4 23l12 7 12-7" stroke="currentColor" stroke-width="2" />
        </svg>
        <span>LayerLens</span>
      </div>
      <div class="header-label">设计的细节，实现的起点</div>
      <span class="stage-badge">工程初始化</span>
    </header>

    <main class="workspace-layout">
      <section class="workspace" aria-labelledby="workspace-title">
        <div class="workspace-toolbar">
          <span id="workspace-title">工作区</span>
          <span class="readonly-label">
            <svg viewBox="0 0 16 16" fill="none" aria-hidden="true">
              <rect x="3.5" y="7" width="9" height="6.5" rx="1.5" stroke="currentColor" />
              <path d="M5.5 7V4.5a2.5 2.5 0 0 1 5 0V7" stroke="currentColor" />
            </svg>
            PSD 只读
          </span>
        </div>

        <div class="workspace-canvas">
          <div class="empty-state">
            <div class="empty-art" aria-hidden="true">
              <div class="art-outline"></div>
              <div class="art-sheet art-sheet-back"></div>
              <div class="art-sheet art-sheet-front">
                <svg viewBox="0 0 64 64" fill="none">
                  <path
                    d="m32 13 22 13-22 13-22-13 22-13Z"
                    stroke="currentColor"
                    stroke-width="2"
                  />
                  <path
                    d="m10 35 22 13 22-13M10 44l22 13 22-13"
                    stroke="currentColor"
                    stroke-width="2"
                  />
                </svg>
                <span>LayerLens</span>
              </div>
              <span class="art-corner art-corner-top"></span>
              <span class="art-corner art-corner-bottom"></span>
            </div>
            <p class="eyebrow">从一个局部开始</p>
            <h1>为设计留一个工作台</h1>
            <p class="empty-description">
              在这里查看 PSD、选择局部，<br />
              让设计信息成为实现的依据。
            </p>
            <div class="development-note">
              <span class="note-dot"></span>
              当前正在搭建基础，尚未开放 PSD 文件读取。
            </div>
          </div>
          <span class="canvas-caption">细节有据，范围清晰</span>
        </div>
      </section>

      <aside class="sidebar" aria-label="应用概览">
        <section class="status-card" aria-labelledby="status-title">
          <div class="section-heading">
            <h2 id="status-title">应用状态</h2>
            <span class="section-index">01</span>
          </div>
          <div class="status-content" aria-live="polite" :aria-busy="status.phase === 'checking'">
            <template v-if="status.phase === 'checking'">
              <span class="status-tag"><span class="status-dot is-checking"></span>正在检查</span>
              <h3>读取桌面应用状态</h3>
              <p>正在确认应用是否已准备就绪。</p>
            </template>
            <template v-else-if="status.phase === 'ready'">
              <span class="status-tag is-ready"><span class="status-dot"></span>桌面应用可用</span>
              <h3>{{ status.info.appName }}</h3>
              <p>应用版本 {{ status.info.appVersion }}。基础通信正常。</p>
            </template>
            <template v-else-if="status.phase === 'browser'">
              <span class="status-tag"><span class="status-dot"></span>浏览器预览</span>
              <h3>请在桌面应用中继续</h3>
              <p>这里仅预览界面，桌面能力需要在 LayerLens 桌面应用中使用。</p>
            </template>
            <template v-else>
              <span class="status-tag is-error"><span class="status-dot"></span>检查未完成</span>
              <h3>暂时无法获取应用状态</h3>
              <p role="alert">{{ status.message }}</p>
            </template>
          </div>
          <button
            v-if="status.phase !== 'browser'"
            class="check-button"
            type="button"
            :disabled="status.phase === 'checking'"
            @click="checkStatus"
          >
            <svg viewBox="0 0 16 16" fill="none" aria-hidden="true">
              <path
                d="M13 5.5A5.5 5.5 0 1 0 13.4 10M13 2v3.5H9.5"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
                stroke-linejoin="round"
              />
            </svg>
            {{ status.phase === 'checking' ? '正在检查…' : '重新检查' }}
          </button>
        </section>

        <section class="purpose-card" aria-labelledby="purpose-title">
          <div class="section-heading">
            <h2 id="purpose-title">关于工作区</h2>
            <span class="section-index">02</span>
          </div>
          <p>LayerLens 是连接 PSD 设计稿与编程 Agent 的只读工作台。</p>
          <div class="principle">
            <span class="principle-line"></span>
            <div>
              <h3>保留原稿</h3>
              <p>查看与选区不修改设计内容。</p>
            </div>
          </div>
          <div class="principle">
            <span class="principle-line"></span>
            <div>
              <h3>逐块实现</h3>
              <p>由你决定范围与推进顺序。</p>
            </div>
          </div>
          <p class="roadmap-note">PSD 解析、素材导出与 Agent 连接将在后续阶段接入。</p>
        </section>
        <p class="sidebar-footnote">让每一次实现，都有原稿可循。</p>
      </aside>
    </main>

    <footer class="app-footer">
      <span>本地工作台 · 只读设计稿</span>
      <span>LayerLens</span>
    </footer>
  </div>
</template>
