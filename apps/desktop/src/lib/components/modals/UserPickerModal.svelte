<script lang="ts">
  import type { JiraStatus, JiraUserSummary } from '$lib/data';
  import { connectionsState } from '$lib/state/connections.svelte';
  import { inboxState } from '$lib/state/inbox.svelte';
  import { modalsState, closeModal } from '$lib/state/modals.svelte';

  interface Props {
    onUserPickerInput: (q: string) => void;
    selectJiraUser: (u: JiraUserSummary | null) => Promise<void> | void;
    selectAnyJiraUser: () => Promise<void> | void;
  }
  let { onUserPickerInput, selectJiraUser, selectAnyJiraUser }: Props = $props();

  const m = $derived(modalsState.userPicker);
  const jiraStatus = $derived<JiraStatus>(connectionsState.jira);
</script>

{#if m}
  <div
    class="modal-backdrop"
    onclick={(e) => { if (e.target === e.currentTarget) closeModal('userPicker'); }}
    onkeydown={(e) => { if (e.key === 'Escape') closeModal('userPicker'); }}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <div class="modal modal-wide">
      <header class="modal-head">
        <svg class="i" viewBox="0 0 24 24" style="color: var(--blue-bright)">
          <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"/>
          <circle cx="12" cy="7" r="4"/>
        </svg>
        <div>
          <div class="modal-title">Choose assignee</div>
          <div class="modal-sub">Filter Jira by any user in the workspace</div>
        </div>
        <button class="modal-close" onclick={() => closeModal('userPicker')} aria-label="Close">
          <svg class="i" viewBox="0 0 24 24"><path d="M18 6 6 18M6 6l12 12" /></svg>
        </button>
      </header>
      <div class="modal-body">
        <!-- svelte-ignore a11y_autofocus -->
        <input
          class="field-input"
          type="search"
          placeholder="Search by name or email…"
          value={m.query}
          oninput={(e) => onUserPickerInput((e.currentTarget as HTMLInputElement).value)}
          autofocus
        />

        <div class="user-list">
          <button class="user-row" class:active={inboxState.jiraAssigneeAny} onclick={() => selectAnyJiraUser()}>
            <span class="user-row-avatar user-row-avatar--any" aria-hidden="true">
              <svg class="i i-sm" viewBox="0 0 24 24">
                <circle cx="9" cy="7" r="4"/><circle cx="17" cy="7" r="3"/>
                <path d="M3 21v-2a4 4 0 0 1 4-4h4a4 4 0 0 1 4 4v2M15 14h2a4 4 0 0 1 4 4v3"/>
              </svg>
            </span>
            <div class="user-row-body">
              <div class="user-row-name">Any user</div>
              <div class="user-row-email">No assignee filter — everyone's tickets</div>
            </div>
            {#if inboxState.jiraAssigneeAny}
              <svg class="i i-sm" viewBox="0 0 24 24" style="color: var(--accent-bright); margin-left:auto;"><path d="M20 6 9 17l-5-5"/></svg>
            {/if}
          </button>
          <button
            class="user-row"
            class:active={inboxState.jiraAssignee === null && !inboxState.jiraAssigneeAny}
            onclick={() => selectJiraUser(null)}
          >
            <span class="chip-avatar" style="width:28px; height:28px; border-radius:50%;"></span>
            <div class="user-row-body">
              <div class="user-row-name">Me (authenticated account)</div>
              <div class="user-row-email">{jiraStatus.kind === 'connected' ? jiraStatus.user.display_name : ''}</div>
            </div>
          </button>

          {#if m.loading}
            <div class="tab-state">Searching…</div>
          {:else if m.error}
            <div class="tab-state tab-state--error">{m.error}</div>
          {:else if m.results.length === 0 && m.query.trim()}
            <div class="tab-state">No users found.</div>
          {:else}
            {#each m.results as u (u.account_id)}
              <button
                class="user-row"
                class:active={inboxState.jiraAssignee?.account_id === u.account_id}
                onclick={() => selectJiraUser(u)}
              >
                <img src={u.avatar_url} alt="" class="user-row-avatar" />
                <div class="user-row-body">
                  <div class="user-row-name">{u.display_name}</div>
                  {#if u.email_address}<div class="user-row-email">{u.email_address}</div>{/if}
                </div>
                {#if inboxState.jiraAssignee?.account_id === u.account_id}
                  <svg class="i i-sm" viewBox="0 0 24 24" style="color: var(--accent-bright); margin-left:auto;"><path d="M20 6 9 17l-5-5"/></svg>
                {/if}
              </button>
            {/each}
          {/if}
        </div>
      </div>
      <footer class="modal-foot">
        <button class="btn btn--ghost" onclick={() => closeModal('userPicker')}>Close</button>
      </footer>
    </div>
  </div>
{/if}
