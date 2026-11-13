import init from './pkg/outfit_web.js';
import { initI18n } from './i18n/index.js';

initI18n();
init().catch((error: unknown) => {
  const status = document.getElementById('startup');
  if (!(status instanceof HTMLElement)) throw error;
  const message = error instanceof Error ? error.message : String(error);
  status.className = 'notice error';
  status.textContent = `无法启动 WebAssembly 编辑器：${message}。请通过 HTTP(S) 静态服务器打开完整的 dist 目录。`;
});
