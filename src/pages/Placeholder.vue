<script setup lang="ts">
// 其余页面先占位：外壳（导航 / 标题 / 右栏）与旧版一致，内容按阶段补齐
import Card from "../components/Card.vue";

defineProps<{ title: string; subtitle: string; notes?: string[] }>();
</script>

<template>
  <div class="page">
    <div class="page-head">
      <div class="titles">
        <h1 class="h1">{{ title }}</h1>
        <div class="muted">{{ subtitle }}</div>
      </div>
    </div>
    <Card :padding="18" :spacing="8">
      <div class="title">这一页还在迁移中</div>
      <div class="muted">
        重构版把外壳（标题栏 / 左导航 / 右栏）与仪表板、日历、设置先对齐，其余页面按
        阶段接入；布局与功能位置会和旧版保持一致。
      </div>
      <ul v-if="notes && notes.length" class="notes">
        <li v-for="note in notes" :key="note">{{ note }}</li>
      </ul>
    </Card>
  </div>
</template>

<style scoped>
.page {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.title {
  font-size: 15px;
  font-weight: 640;
}
.muted {
  font-size: 13px;
  line-height: 1.65;
}
.notes {
  margin: 4px 0 0;
  padding-left: 18px;
  color: var(--muted);
  font-size: 12.5px;
  line-height: 1.7;
}
</style>
