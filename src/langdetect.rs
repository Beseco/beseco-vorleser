//! Erkennt die Sprache eines Textes an häufigen kleinen Wörtern (der, the, le, el …), ohne KI und
//! ohne Wörterbuch. Genug, um zur passenden Stimme zu greifen.

/// Primäre Sprachkennungen (LANGID & 0x3ff) und ihre häufigsten Wörter.
const LANGUAGES: [(u16, &str); 6] = [
    (0x07, "der die das und ist nicht ein eine einen zu mit von den sich auf für dem des im auch es ich sie wir werden wird sind war wie aber oder wenn noch nach bei aus nur kann"),
    (0x09, "the and is of to in that it for you was with on are as be this have not but or at by from they we will can an which has would there their"),
    (0x0C, "le la les et est des une un du que qui dans pour pas sur avec au ce il elle nous vous sont par plus mais ou cette"),
    (0x0A, "el los las y es que en un una por con para no se del al lo como más pero sus está son muy también"),
    (0x10, "il gli e è di che un una per con non del della sono come più ma anche nel alla questo"),
    (0x13, "het een en is van dat niet op te zijn met voor er maar ook als bij naar wordt heeft"),
];

/// Die Sprache von `text` als primäre Sprachkennung, wenn sie klar erkennbar ist.
pub fn detect(text: &str) -> Option<u16> {
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| !w.is_empty())
        .take(400)
        .map(str::to_lowercase)
        .collect();
    if words.len() < 12 {
        return None;
    }
    let mut scores: Vec<(u16, usize)> = LANGUAGES
        .iter()
        .map(|(lang, list)| {
            let list: Vec<&str> = list.split(' ').collect();
            (*lang, words.iter().filter(|w| list.contains(&w.as_str())).count())
        })
        .collect();
    scores.sort_by(|a, b| b.1.cmp(&a.1));
    let (best, hits) = scores[0];
    let second = scores[1].1;
    // Deutlich vorn und häufig genug, sonst lieber nichts ändern.
    (hits * 100 >= words.len() * 12 && hits * 10 >= second * 14).then_some(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_languages() {
        assert_eq!(detect("Die Stadtwerke Freising planen für das kommende Jahr umfangreiche Arbeiten an der Wasserversorgung, und alle Haushalte werden informiert."), Some(0x07));
        assert_eq!(detect("The city of Freising is planning extensive work on the water supply next year, and all households will be informed in advance."), Some(0x09));
        assert_eq!(detect("La ville de Freising prévoit des travaux importants sur le réseau d'eau, et les habitants seront informés par courrier."), Some(0x0C));
        assert_eq!(detect("La ciudad de Freising planea obras en la red de agua para el próximo año y los vecinos recibirán una carta con más información."), Some(0x0A));
        // Zu kurz oder ohne klare Merkmale: keine Aussage.
        assert_eq!(detect("Hallo Welt"), None);
        assert_eq!(detect("Freising München Augsburg Regensburg Nürnberg Würzburg Bamberg Passau Landshut Ingolstadt Rosenheim Kempten"), None);
    }
}
