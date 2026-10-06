# Environnement de développement — helper CLI

> [!info] Deux modes
> - **Minimal** : `.devcontainer/` seul → Rust, Node, SQLite (pas d'outils perso).
> - **Perso** : overlay `my-env/` → ajoute zsh/Oh My Zsh, nvim, lazygit + configs bind-montées.

## Prérequis

- Docker Desktop
- CLI `devcontainer` (npm : `@devcontainers/cli`) — v0.88.0 testé

> [!warning] Le CLI `devcontainer` n'a **pas** de sous-commande `stop`.
> Pour arrêter un conteneur, utiliser `docker stop` avec le label `devcontainer.local_folder` (voir ci-dessous).

## Mode minimal (`.devcontainer/` seul)

```bash
# Créer / démarrer le conteneur
devcontainer up --workspace-folder .

# Ouvrir un shell (bash — pas de zsh en mode minimal)
devcontainer exec --workspace-folder . bash

# (Re)construire uniquement l'image (sans lancer le conteneur)
devcontainer build --workspace-folder .

# Inspecter la config résolue
devcontainer read-configuration --workspace-folder .

# Arrêter le conteneur
docker stop "$(docker ps -q --filter "label=devcontainer.local_folder=$(pwd)")"

# Recréer le conteneur de zéro (supprime l'existant + rebuild si besoin)
devcontainer up --workspace-folder . --remove-existing-container
```

## Mode perso (`my-env/` overlay)

```bash
# Build base + overlay + démarrage + link.sh (configs perso)
./my-env/up.sh

# Rebuild complet des deux images (sans cache)
./my-env/up.sh --no-cache

# Recréer le conteneur
./my-env/up.sh --remove-existing-container

# Ouvrir un shell perso (zsh + Oh My Zsh)
devcontainer exec --workspace-folder . zsh

# Arrêter le conteneur
./my-env/down.sh
```

## Ports forwardés

| Port | Service |
|---|---|
| `5173` | SvelteKit (dev) |
| `3000` | Axum (backend) |

## Commandes utiles

```bash
# Voir les conteneurs devcontainer
docker ps -a --filter "label=devcontainer.local_folder=$(pwd)"

# Suivre les logs d'un conteneur
docker logs -f "$(docker ps -q --filter "label=devcontainer.local_folder=$(pwd)")"

# Supprimer le conteneur (arrêt + destruction)
docker rm -f "$(docker ps -aq --filter "label=devcontainer.local_folder=$(pwd)")"
```
