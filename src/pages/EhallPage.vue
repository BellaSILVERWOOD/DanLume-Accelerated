<script setup lang="ts">
// eHall（复旦网上办事大厅）：**内嵌网页**，登录状态会保留（对应旧版 page_ehall.py）。
// 以前是「打开网页」弹独立窗口，这里改成贴在页面里的子 Webview。
import { onMounted, ref } from "vue";
import Card from "../components/Card.vue";
import EmbedWebview from "../components/EmbedWebview.vue";
import { settingsAll, settingsSave } from "../api";

const EHALL_URL = "https://ehall.fudan.edu.cn/";
const url = ref(EHALL_URL);
const loaded = ref(false);

onMounted(async () => {
  try {
    const all = await settingsAll();
    const stored = (all.values || {})["ehall_url"] || "";
    url.value = stored.trim() || EHALL_URL;
  } catch {
    url.value = EHALL_URL;
  }
  loaded.value = true;
});

/** 设置里改过 ehall_url 的话，这里也跟着用（老版本留的键） */
async function remember(next: string) {
  url.value = next;
  await settingsSave({ ehall_url: next });
}
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="titles">
        <h1 class="h1">eHall</h1>
        <div class="muted">复旦网上办事大厅 · 内嵌网页，登录状态会保留，过期就在网页里重新登录</div>
      </div>
      <div class="spacer" />
      <button class="ghost" title="把当前地址记进设置" @click="remember(url)">记住这个地址</button>
    </div>

    <Card :padding="10" class="ehall-card">
      <EmbedWebview v-if="loaded" kind="ehall" :url="url" height="100%" home-label="回到首页" />
    </Card>
  </div>
</template>

<style scoped>
.page {
  display: flex;
  flex-direction: column;
  gap: 10px;
  height: 100%;
}
.ehall-card {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
}
.ghost {
  border: 1px solid var(--border-soft);
  background: var(--card);
  color: var(--muted);
  border-radius: 999px;
  padding: 5px 12px;
  font-size: 12px;
  font-family: inherit;
  cursor: pointer;
}
.ghost:hover {
  color: var(--accent);
  border-color: var(--accent);
}
</style>
