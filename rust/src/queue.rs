//! Regras puras de navegação da fila, sem arquivo, device ou thread.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueAction {
    Load(usize),
    Restart,
    Stop,
}

/// Limite do padrão de player: antes disso, Previous navega; a partir daqui, rebobina.
pub const RESTART_THRESHOLD_SECONDS: f64 = 3.0;

pub fn on_next(current: Option<usize>, len: usize) -> Option<QueueAction> {
    let next = current?.checked_add(1)?;
    (next < len).then_some(QueueAction::Load(next))
}

pub fn on_previous(current: Option<usize>, elapsed: f64) -> Option<QueueAction> {
    let current = current?;
    if elapsed >= RESTART_THRESHOLD_SECONDS {
        Some(QueueAction::Restart)
    } else {
        current.checked_sub(1).map(QueueAction::Load)
    }
}

/// Vários cliques em Next/Previous somados num deslocamento só, para a interface carregar
/// uma faixa em vez de uma por clique. Um passo é exatamente `on_next`/`on_previous`.
pub fn on_navigate(
    current: Option<usize>,
    len: usize,
    elapsed: f64,
    steps: i32,
) -> Option<QueueAction> {
    let current = current?;
    if steps > 0 {
        // O primeiro passo decide se há para onde ir; os demais só somam até a última.
        on_next(Some(current), len)?;
        let last = len - 1;
        return Some(QueueAction::Load(
            current.saturating_add(steps as usize).min(last),
        ));
    }
    if steps < 0 {
        let back = steps.unsigned_abs() as usize;
        return match on_previous(Some(current), elapsed)? {
            // Passados 3 s, o primeiro passo para trás só rebobina a faixa atual.
            QueueAction::Restart if back == 1 || current == 0 => Some(QueueAction::Restart),
            QueueAction::Restart => Some(QueueAction::Load(current.saturating_sub(back - 1))),
            _ => Some(QueueAction::Load(current.saturating_sub(back))),
        };
    }
    None
}

pub fn on_finished(current: Option<usize>, len: usize) -> Option<QueueAction> {
    let current = current?;
    if current >= len {
        return None;
    }
    Some(match current.checked_add(1) {
        Some(next) if next < len => QueueAction::Load(next),
        _ => QueueAction::Stop,
    })
}

pub fn next_playable(from: usize, failed: &[bool], len: usize) -> Option<usize> {
    (from..len).find(|&index| !failed.get(index).copied().unwrap_or(false))
}

pub fn after_removal(
    current: Option<usize>,
    removed: usize,
    len_after: usize,
) -> (Option<usize>, bool) {
    let Some(current) = current else {
        return (None, false);
    };
    if removed < current {
        return (Some(current - 1), false);
    }
    if removed > current {
        return (Some(current), false);
    }
    if current < len_after {
        (Some(current), true)
    } else {
        (None, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_navega_ate_a_ultima_e_para_nas_pontas() {
        assert_eq!(on_next(None, 0), None);
        assert_eq!(on_next(None, 3), None);
        assert_eq!(on_next(Some(0), 3), Some(QueueAction::Load(1)));
        assert_eq!(on_next(Some(1), 3), Some(QueueAction::Load(2)));
        assert_eq!(on_next(Some(2), 3), None);
        assert_eq!(on_next(Some(9), 3), None);
    }

    #[test]
    fn previous_navega_antes_do_limite_e_reinicia_a_partir_dele() {
        assert_eq!(on_previous(None, 0.0), None);
        assert_eq!(on_previous(Some(0), 2.999), None);
        assert_eq!(on_previous(Some(1), 2.999), Some(QueueAction::Load(0)));
        assert_eq!(on_previous(Some(0), 3.0), Some(QueueAction::Restart));
        assert_eq!(on_previous(Some(2), 4.0), Some(QueueAction::Restart));
    }

    #[test]
    fn finished_avanca_ou_para_sem_sair_do_intervalo() {
        assert_eq!(on_finished(None, 0), None);
        assert_eq!(on_finished(None, 2), None);
        assert_eq!(on_finished(Some(0), 2), Some(QueueAction::Load(1)));
        assert_eq!(on_finished(Some(1), 2), Some(QueueAction::Stop));
        assert_eq!(on_finished(Some(9), 2), None);
    }

    #[test]
    fn next_playable_pula_falhas_consecutivas() {
        let failed = [false, true, true, false];
        assert_eq!(next_playable(0, &failed, 4), Some(0));
        assert_eq!(next_playable(1, &failed, 4), Some(3));
        assert_eq!(next_playable(4, &failed, 4), None);
        assert_eq!(next_playable(0, &[true, true], 2), None);
        assert_eq!(next_playable(0, &[], 0), None);
    }

    #[test]
    fn removal_ajusta_indice_e_indica_quando_recarregar() {
        assert_eq!(after_removal(None, 0, 0), (None, false));
        assert_eq!(after_removal(Some(2), 0, 2), (Some(1), false));
        assert_eq!(after_removal(Some(1), 2, 2), (Some(1), false));
        assert_eq!(after_removal(Some(1), 1, 2), (Some(1), true));
        assert_eq!(after_removal(Some(2), 2, 2), (None, true));
        assert_eq!(after_removal(Some(0), 0, 0), (None, true));
    }

    #[test]
    fn um_passo_equivale_a_next_e_previous() {
        for current in 0..4 {
            for elapsed in [0.0, 5.0] {
                assert_eq!(
                    on_navigate(Some(current), 4, elapsed, 1),
                    on_next(Some(current), 4)
                );
                assert_eq!(
                    on_navigate(Some(current), 4, elapsed, -1),
                    on_previous(Some(current), elapsed)
                );
            }
        }
    }

    #[test]
    fn varios_passos_para_frente_somam_e_param_na_ultima() {
        assert_eq!(on_navigate(Some(0), 10, 0.0, 3), Some(QueueAction::Load(3)));
        assert_eq!(on_navigate(Some(7), 10, 0.0, 5), Some(QueueAction::Load(9)));
        assert_eq!(on_navigate(Some(9), 10, 0.0, 2), None);
    }

    #[test]
    fn varios_passos_para_tras_somam_e_param_na_primeira() {
        assert_eq!(
            on_navigate(Some(5), 10, 0.0, -3),
            Some(QueueAction::Load(2))
        );
        assert_eq!(
            on_navigate(Some(1), 10, 0.0, -4),
            Some(QueueAction::Load(0))
        );
        assert_eq!(on_navigate(Some(0), 10, 0.0, -2), None);
    }

    #[test]
    fn so_o_primeiro_passo_para_tras_reinicia_a_faixa() {
        // Passados 3 s, o primeiro Previous rebobina e os seguintes navegam: dois cliques
        // voltam uma faixa, como fariam dois comandos separados.
        assert_eq!(
            on_navigate(Some(5), 10, 4.0, -2),
            Some(QueueAction::Load(4))
        );
        assert_eq!(
            on_navigate(Some(5), 10, 4.0, -3),
            Some(QueueAction::Load(3))
        );
        assert_eq!(
            on_navigate(Some(0), 10, 4.0, -3),
            Some(QueueAction::Restart)
        );
    }

    #[test]
    fn zero_passos_ou_fila_sem_faixa_nao_fazem_nada() {
        assert_eq!(on_navigate(Some(3), 10, 0.0, 0), None);
        assert_eq!(on_navigate(None, 10, 0.0, 1), None);
        assert_eq!(on_navigate(None, 10, 0.0, -1), None);
    }
}
