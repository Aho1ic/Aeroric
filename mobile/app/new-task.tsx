/** 新建任务整页:表单实现见 src/components/NewTaskSheet.tsx(与首页卡片 ＋ 弹窗共用)。 */

import { Stack, router } from "expo-router";
import { NewTaskPage } from "../src/components/NewTaskSheet";
import { t } from "../src/i18n";

export default function NewTaskScreen() {
  return (
    <>
      <Stack.Screen options={{ title: t("nav.newTask") }} />
      <NewTaskPage onClose={() => router.back()} />
    </>
  );
}
