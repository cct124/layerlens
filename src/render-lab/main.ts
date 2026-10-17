import { createApp } from 'vue';
import RenderLab from './RenderLab.vue';
import './style.css';
const boot: unknown = window.__LAYERLENS_DEV_BOOT__;
if (boot && typeof boot === 'object' && 'renderLab' in boot && boot.renderLab === true)
  createApp(RenderLab).mount('#app');
else document.body.textContent = '渲染实验只允许显式隔离开发实例启动。';
