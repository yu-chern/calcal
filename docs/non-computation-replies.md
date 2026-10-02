# 非计算回答与计算边界

## 原问题与行为

此前强制所有请求进入compute或clarify，导致“Hi”也被当成无法计算的待补充条件。现在保留单一有界Agent循环，增加respond v1终止接口：问候、能力介绍、已覆盖概念及超范围说明正常完成；真正影响计算答案的歧义仍由clarify暂停。

## 接口与验证

respond参数只有topics、program、reference，不允许自由文本答案。topics最多4个，后端按固定枚举读取版本化内容。范围限于现有条目；未覆盖的概念给出可解释主题，不伪装成开放百科能力。知识内容见src/tools/reply.rs；闰年与公历/中国农历条目的事实依据是[美国海军天文台历法说明](https://aa.usno.navy.mil/faq/calendars)。其余条目说明数学定义与实际工具口径。

| 请求 | 路径 | 最终证据 |
| --- | --- | --- |
| Hi、致谢、能力介绍 | respond(topics)，正常完成 | response_verified，knowledge ID/版本/文本 |
| 闰年是什么、日期差如何理解 | respond(topics)，正常完成 | 相同知识证据，不生成用户特定日期结论 |
| 判断2024是否闰年 | compute程序 | answer_verified与成功tool_result |
| 解释闰年并判断2024 | respond(topics, program) | 同一运行时执行程序，answer_verified含claims和knowledge |
| 解释刚才怎么算的 | respond(reference) | response_verified引用原run_id、call_id与claims |
| 最近是哪年但未指定方向 | clarify | clarification，等待条件并支持续接 |
| 农历转换、微积分等不可执行任务 | respond(scope) | 正常范围说明，不进入等待状态 |

program格式与compute v2完全相同，原文绑定、禁止模型数字字面量、精确有理数、执行和结果大小/超时边界均保留。任一步失败时，不呈现混合回答的部分知识或模型文字。respond工作量为程序步骤数加1，纳入原Agent预算。成功后服务器直接呈现工具文本，无须第二次LLM生成。

reference由服务器提供，查询最近3个同会话、当前请求之前、已完成计算；必须有answer_verified以及对应call_id的成功完整tool_result。模型只收到引用key和截断的结果摘要，工具读取完整步骤与结果。跨会话、失败、未完成及模型助手文本均不能作为计算证据。历史解释采用步骤类别和原验证文本，不公开模型内部推理；它解释执行记录，不证明模型选对了公式。

新增响应事件保留真实执行记录，UI已有completed状态正常显示；无需新增前端分支。移除生产clarify的unsupported主题，避免把缺少能力误当成待补充条件。没有新增数据库迁移，也没有开放代码、网络或文件执行。

## 限制与验证

模型仍负责选择正确主题、公式和条件，可能漏掉请求的一部分。协议确保展示的派生结论有成功工具证据，不提供任意自然语言理解的正确性证明。知识回答采用受限条目，因此开放解释的灵活性低于自由生成；新增主题需审核后加入后端与schema。

离线测试覆盖闭合主题、自由文本拒绝、混合程序失败、历史证据失败与工作量计数；隔离数据库覆盖正常终止、证据事件、同会话引用/跨会话拒绝，兼容此前计算拒绝与澄清续接。真实模型评估包括问候、概念、超范围、混合计算及历史追问，并回归日期/数学/歧义场景。评估记录见[验证JSON](validation/non-computation-replies.json)。生产验收与回退见[部署记录](deployment.md)。
