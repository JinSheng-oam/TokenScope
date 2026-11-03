<script lang="ts">
import { formatNumber, percentOf } from '../lib/format';
  import type { DashboardSnapshot, UsageTotals } from '../lib/types';

  export let snapshot: DashboardSnapshot;
  export let local: UsageTotals | undefined;
</script>

<div class="page-header">
            <h1>项目分析</h1>
            <p>基于脱敏项目标识符归集；源码路径与仓库名留在本地，绝不上传</p>
          </div>

          <section class="panel-card">
            {#if snapshot.by_project.length > 0}
              <div class="table-wrap">
                <table>
                  <thead>
                    <tr>
                      <th>项目名称</th>
                      <th>Token 消耗量</th>
                      <th>用量记录数</th>
                      <th>全机份额</th>
                    </tr>
                  </thead>
                  <tbody>
                    {#each snapshot.by_project as prj}
                      <tr>
                        <td>{prj.label}</td>
                        <td class="tabular">{formatNumber(prj.total_tokens)}</td>
                        <td class="tabular">{formatNumber(prj.record_count)}</td>
                        <td>
                          <div class="rank-track" style="width: 120px;">
                            <div class="rank-fill" style={`width: ${percentOf(prj.total_tokens, local?.total_tokens ?? 0)}%`}></div>
                          </div>
                        </td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            {:else}
              <div class="empty-box">当前本地来源尚未解析出项目标识</div>
            {/if}
          </section>
