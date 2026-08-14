{
  config,
  pkgs,
  lib,
  ...
}: {
  # Drop-in replacement for /etc/nixos/modules/hardware/nvidia-ollama.nix
  #
  # Tuned for a single RTX 3050 6GB Laptop GPU running qwen3:4b. Every value
  # below is set because of a measurement recorded in
  # docs/research/2026-08-10-qwen3-4b-thinking.md — not by convention.

  hardware.nvidia = {
    nvidiaSettings = lib.mkDefault true;
    # Keeping the GPU out of low-power states avoids the first-token stall that
    # power management introduces on laptop Ampere parts.
    powerManagement.enable = lib.mkDefault false;
    prime.offload.enableOffloadCmd = lib.mkDefault false;
  };

  hardware.graphics.enable = true;

  services.ollama = {
    enable = true;
    package = pkgs.ollama-cuda;

    # Pull on activation so a rebuild never leaves you without the model.
    loadModels = ["qwen3:4b"];

    environmentVariables = {
      # Flash attention is a prerequisite for KV-cache quantisation; without it
      # OLLAMA_KV_CACHE_TYPE is silently ignored and you still OOM.
      OLLAMA_FLASH_ATTENTION = "1";

      # Halves KV-cache memory at negligible quality cost. On 6GB this is the
      # difference between a 12k window fully on-GPU and a 24k one.
      OLLAMA_KV_CACHE_TYPE = "q8_0";

      # 6GB holds exactly one 4B model plus its cache. Loading a second evicts
      # the first mid-conversation; parallel slots split one context window
      # into N smaller ones, which silently truncates long evidence packets.
      OLLAMA_MAX_LOADED_MODELS = "1";
      OLLAMA_NUM_PARALLEL = "1";

      # Reloading qwen3:4b costs ~2s of GPU transfer. An agent loop makes many
      # short calls, so keep the model resident between them.
      OLLAMA_KEEP_ALIVE = "30m";

      # Server-side default window. Measured: 12288 keeps all 37/37 layers on
      # the GPU at ~52 tok/s; 24576+ spills 14 layers to CPU and halves that.
      # With the q8_0 cache above you can try 24576 and re-check the offload
      # line in `journalctl -u ollama | grep offloaded`.
      OLLAMA_CONTEXT_LENGTH = "12288";
    };
  };

  environment.systemPackages = with pkgs; [
    cudatoolkit
    # `nvtop` shows VRAM headroom live, which is how you check whether a
    # context size still fits before committing to it.
    nvtop
  ];
}
