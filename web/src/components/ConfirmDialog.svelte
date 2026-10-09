<script lang="ts">
  import * as AlertDialog from "$lib/components/ui/alert-dialog";
  import { confirm } from "../state/confirm.svelte";
</script>

<AlertDialog.Root bind:open={() => confirm.pending !== null, (open) => !open && confirm.settle(false)}>
  <AlertDialog.Content>
    <AlertDialog.Header>
      <AlertDialog.Title>{confirm.pending?.title}</AlertDialog.Title>
      {#if confirm.pending?.description}
        <AlertDialog.Description>{confirm.pending.description}</AlertDialog.Description>
      {/if}
    </AlertDialog.Header>
    <AlertDialog.Footer>
      <AlertDialog.Cancel>取消</AlertDialog.Cancel>
      <AlertDialog.Action variant="destructive" onclick={() => confirm.settle(true)}>
        {confirm.pending?.action}
      </AlertDialog.Action>
    </AlertDialog.Footer>
  </AlertDialog.Content>
</AlertDialog.Root>
