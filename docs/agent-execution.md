> 本文记录compute v1历史优化。当前生产协议升级说明见[来源绑定计算运行时](verified-computation.md)，实际部署状态见[部署记录](deployment.md)。

# 日期与计算 Agent 执行优化（2026-10-01）

适用范围是已有公历日期、数学表达式与两者组合的任务。目标是在确定口径后减少模型往返，并由确定性工具验证计算；不承诺任意自然语言请求都能全局最优执行或保证模型永不误解。代码 dee6d5a 已于2026-10-01重启部署到公网；生产入口及身份保护检查通过，获准身份的UI端到端验收待用户登录完成。

## 执行策略

- 直接查询实际日期：读取本轮服务器时间/时区上下文，无须单独 today 往返。
- 一个表达式或日期操作：calculator/calendar。
- 多个独立操作或有依赖的组合：compute 一次提交最小计划，Rust 顺序执行并通过有类型的 JSON pointer 引用传递中间结果。
- 条件日期搜索：find_dates 按过去、未来或双向距离遍历，明确参考日是否包含、搜索天数和返回数量，检查所有更近候选再返回。支持月份、月日、星期、闰年、月末条件。双向等距离结果保留并列；不能替用户选边。
- 歧义、冲突、缺项或能力不足：clarify 直接保存问题并结束本轮，等待回复，无须再调用模型复述。澄清的问题/选项只描述规则，不提前计算未经验证的候选答案。

## 组合输入示例

以下为工具接口示例，不是历史抓包：

```json
{
  "steps": [
    {
      "id": "span",
      "action": {
        "kind": "calendar",
        "operation": "diff",
        "date": null,
        "start": "2026-10-01",
        "end": "2026-10-15",
        "amount": null,
        "unit": null
      }
    },
    {
      "id": "cost",
      "action": {
        "kind": "calculate",
        "expression": "days * rate",
        "variables": [
          {
            "name": "days",
            "value": {
              "ref": "/span/days"
            }
          },
          {
            "name": "rate",
            "value": 120
          }
        ]
      }
    }
  ]
}
```

返回保留每一步的 id、action、resolved_action、ok、data/error。上述日期差得到14后作为 days 传入公式，得到1680；不需要模型在两次工具调用之间搬运数值。任一步失败即停止后续步骤，complete=false，顶层 ok=false/error.code=PLAN_FAILED，已执行结果与 skipped_steps 保留。

引用只允许此前成功步骤的数值或字符串。拒绝前向引用、缺失字段、重复 ID、未知字段、过深嵌套、非整数日期偏移及超出范围的参数。没有任意脚本、文件或网络执行。

## 澄清的状态与续接

本轮 `runs.status=completed` 表示这一次模型运行结束，`reason=clarification` 和 `activity=等待澄清` 表示原问题尚未解答，`assistant_message` 为澄清问题。GET 接口无需新增状态或端点。UI 在最新澄清消息上显示“等待你补充条件”，允许继续输入；回复后旧消息不再显示等待状态。

同一个模型响应中同时请求澄清与计算时，后端优先执行一个澄清，其他调用记录为 tool_skipped，不执行。无效澄清参数可以在预算内交回模型修正；其他调用返回未执行标记，保持 call_id 对齐。

clarification 事件单独保存原始消息上下文。后续新 run 优先恢复最新未解决的澄清链，包括原问题、之前澄清问答和当前补充，即使 history_turns 很小也不静默丢弃条件。完整上下文仍受总字符预算保护。任务解答后，恢复普通最近交流历史规则。

## 边界

- compute 最多16个子步骤；默认工具工作预算12，按子步骤计数，不允许批处理绕过预算。超限在执行前停止。模型仍有8步、90秒等既有边界。
- 每个日期搜索最多146097天距离；支持公历年份0001—9999。最近搜索在命中目标数量后停止，双向并列截止候选均返回。无结果只表示实际范围内无解，不是无限范围的证明。
- 日期搜索循环主动 yield，允许工具与 run 超时取消。未新增自动网络重试或写入工具。
- 数学仍是有限双精度近似计算，没有新增任意精度、大整数精确运算或会计级小数支持；超出能力的精度要求先澄清。日期差仍是日历日期差，不能冒充跨夏令时的实际小时数。节假日、地区工作日历、个人日程及农历仍不支持。
- 自然语言条件识别与是否调用 clarify 仍由模型判断，后端只能强制执行已请求的澄清优先级与工具边界。有限样本通过不等于所有歧义都必然被识别。

## 验证结果

`scripts/check.sh`：Rust fmt/check/test/clippy 全通过；20个离线 Rust 测试；前端 format/lint、5个交互测试、类型检查与生产构建通过。依赖 nom 1.2.4 的既有未来兼容警告仍存在。

`scripts/test-db.sh`：隔离 calcal_test 集成测试通过，验证旧 API/身份边界、两次模型一次外层工具完成组合任务、子步骤预算、混合澄清优先级，以及 history_turns=1 时连续澄清后保留原条件继续执行。未连接生产数据库。

真实模型回归：gpt-6-luna / medium，10种场景通过；随后对更新后的澄清规则额外复验3种场景通过。各场景最近一次记录如下；秒数是测试轮询测得的 run 完成用时，包含模型、执行器、存储及轮询开销，不能当作供应商精确延迟或稳定 SLA。

| 场景 | LLM 次数 | 外层工具 | 用时（秒） | 结果 |
| --- | --- | --- | --- | --- |
| today | 1 | 无 | 1.34 | completed，通过 |
| arithmetic | 2 | calculator | 2.24 | completed，通过 |
| batch_math | 2 | compute | 3.34 | completed，通过 |
| date_math | 2 | compute | 3.55 | completed，通过 |
| date_search | 2 | compute | 2.83 | completed，通过 |
| ambiguous | 1 | clarify | 2.41 | clarification，通过 |
| conflicting | 1 | clarify | 1.97 | clarification，通过 |
| month_end | 1 | clarify | 1.99 | clarification，通过 |
| precision | 1 | clarify | 6.38 | clarification，通过 |
| clarification_resume | 2 | compute | 3.34 | completed，通过 |

真实调用证明新 strict schema 可被模型 API 接受；批量任务使用一个 compute，日期与数学组合通过引用传值，日期搜索 limit=1，明确任务2次模型调用，澄清1次模型调用。当前日期场景1次模型调用、0工具。与旧截图的3/6/5次模型调用相比，这些是改动后的能力证据，但不同 prompt、运行时间和模型采样不能构成严格同条件延迟基准。

窄屏验收：在375×812视口以本地模拟 API 验证澄清标签、完整问题、可编辑输入框、中文补充、刷新后草稿保留、同会话发送及旧等待标记消失；页面滚动宽度与视口均为375，无横向溢出。使用模拟数据完成 UI 检查，不冒充公网/真实模型端到端验收。中文 IME、发送中防重、失败提示和草稿保留由现有及新增前端自动测试覆盖。

可复验命令：

```bash
./scripts/check.sh
./scripts/test-db.sh
CALCAL_LIVE_EVAL=1 cargo test --locked --test agent_live -- --ignored --nocapture
# 仅复验指定场景；clarification_resume 需要同时选择 ambiguous 作为前序轮次
CALCAL_LIVE_EVAL=1 CALCAL_LIVE_CASES=ambiguous,clarification_resume cargo test --locked --test agent_live -- --ignored --nocapture
```

真实模型测试消耗额度，使用本机服务端配置；必须指定不同于生产 DATABASE_URL 且名称以 _test 结尾的 TEST_DATABASE_URL。结果只打印合成测试输入、可见回答和工具数据，不打印密钥或加密推理。

## 部署和兼容

已通过 scripts/service.sh restart prod 部署 dee6d5a；原服务及专用Tunnel已重启，入口和无令牌保护实测通过。当前验收浏览器没有有效Access登录会话，尚未从获准身份完成本次生产UI测试。无 SQL 迁移，既有 JSONB 历史保持不变。新 snapshot.schema_version=2 保存最终 instructions 与 runtime_context；工具事件增加 step/work_units。旧架构报告和已导出的历史 I/O 属于原代码快照，不应当作这次改动后的运行契约。部署与回退边界见 [部署状态](deployment.md)。
