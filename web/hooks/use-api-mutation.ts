import { useMutation, useQueryClient } from "@tanstack/react-query";

interface UseApiMutationOptions<TData> {
  invalidateKey?: readonly unknown[];
  onSuccess?: (data: TData) => void;
}

export function useApiMutation<TData, TVars>(
  mutationFn: (vars: TVars) => Promise<TData>,
  options: UseApiMutationOptions<TData> = {}
) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn,
    onSuccess: (data) => options.onSuccess?.(data),
    onSettled: () => {
      if (options.invalidateKey) {
        qc.invalidateQueries({ queryKey: options.invalidateKey });
      }
    },
  });
}
