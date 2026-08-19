//! Regras da máquina de estados do player, isoladas de qualquer efeito no hardware.
//! Manter estas regras puras é o que torna possível cobrir todos os pares estado × comando
//! sem device, sem arquivo e sem thread.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerState {
    Idle,
    Loaded,
    Playing,
    Paused,
    Finished,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Load,
    Play,
    Pause,
    Stop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransitionError {
    pub message: &'static str,
}

pub fn next_state(
    current: PlayerState,
    command: Command,
) -> Result<PlayerState, TransitionError> {
    use Command::*;
    use PlayerState::*;

    match (current, command) {
        // Carregar substitui o que estiver carregado, venha de onde vier.
        (_, Load) => Ok(Loaded),
        // Encerrar sempre devolve ao repouso.
        (_, Stop) => Ok(Idle),

        (Loaded | Paused | Finished, Play) => Ok(Playing),
        // Idempotente de propósito: a interface tem um botão só, e um duplo clique não pode
        // virar mensagem de erro.
        (Playing, Play) => Ok(Playing),
        (Idle, Play) => Err(TransitionError {
            message: "nenhum arquivo carregado",
        }),
        (Failed, Play) => Err(TransitionError {
            message: "houve uma falha; carregue o arquivo de novo",
        }),

        (Playing, Pause) => Ok(Paused),
        (Paused, Pause) => Ok(Paused),
        (Idle | Loaded | Finished | Failed, Pause) => Err(TransitionError {
            message: "não está tocando",
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_e_aceito_em_qualquer_estado() {
        for estado in [
            PlayerState::Idle,
            PlayerState::Loaded,
            PlayerState::Playing,
            PlayerState::Paused,
            PlayerState::Finished,
            PlayerState::Failed,
        ] {
            assert_eq!(
                next_state(estado, Command::Load),
                Ok(PlayerState::Loaded),
                "load deveria ser aceito em {estado:?}"
            );
        }
    }

    #[test]
    fn stop_volta_para_idle_de_qualquer_estado() {
        for estado in [
            PlayerState::Idle,
            PlayerState::Loaded,
            PlayerState::Playing,
            PlayerState::Paused,
            PlayerState::Finished,
            PlayerState::Failed,
        ] {
            assert_eq!(next_state(estado, Command::Stop), Ok(PlayerState::Idle));
        }
    }

    #[test]
    fn play_toca_a_partir_de_loaded_paused_e_finished() {
        for estado in [PlayerState::Loaded, PlayerState::Paused, PlayerState::Finished] {
            assert_eq!(
                next_state(estado, Command::Play),
                Ok(PlayerState::Playing),
                "play deveria tocar a partir de {estado:?}"
            );
        }
    }

    #[test]
    fn play_sem_arquivo_carregado_e_erro() {
        assert!(next_state(PlayerState::Idle, Command::Play).is_err());
        assert!(next_state(PlayerState::Failed, Command::Play).is_err());
    }

    #[test]
    fn play_durante_a_reproducao_e_idempotente() {
        assert_eq!(
            next_state(PlayerState::Playing, Command::Play),
            Ok(PlayerState::Playing)
        );
    }

    #[test]
    fn pause_so_faz_sentido_tocando_ou_ja_pausado() {
        assert_eq!(
            next_state(PlayerState::Playing, Command::Pause),
            Ok(PlayerState::Paused)
        );
        assert_eq!(
            next_state(PlayerState::Paused, Command::Pause),
            Ok(PlayerState::Paused)
        );
        for estado in [
            PlayerState::Idle,
            PlayerState::Loaded,
            PlayerState::Finished,
            PlayerState::Failed,
        ] {
            assert!(
                next_state(estado, Command::Pause).is_err(),
                "pause deveria falhar em {estado:?}"
            );
        }
    }

    #[test]
    fn o_erro_traz_mensagem_util() {
        let erro = next_state(PlayerState::Idle, Command::Play).unwrap_err();
        assert!(
            erro.message.contains("carregado"),
            "mensagem pouco informativa: {}",
            erro.message
        );
    }
}
