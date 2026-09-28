#!/usr/bin/env python3
from __future__ import annotations

# Priority units for the high-intelligibility Windows voice bank.
# Runtime keys remain the original HII/navigation text; spoken guidance may be
# localized for clarity while common firmware labels retain their literal name.

LETTER_TEXT = {
    "a":"a","b":"bé","c":"cé","d":"dé","e":"e","f":"èf","g":"gé","h":"ache",
    "i":"i","j":"ji","k":"ka","l":"elle","m":"ème","n":"ène","o":"o","p":"pé",
    "q":"ku","r":"ère","s":"esse","t":"té","u":"u","v":"vé","w":"double vé",
    "x":"ixe","y":"i grec","z":"zède",
}
DIGIT_TEXT = {
    "0":"zéro","1":"un","2":"deux","3":"trois","4":"quatre","5":"cinq",
    "6":"six","7":"sept","8":"huit","9":"neuf",
}
PRIORITY_WORDS = (
    "advanced","action","asus","back","bios","boot","button","change","checked",
    "configuration","cpu","default","device","disabled","enabled","enter","escape",
    "exit","help","left","main","memory","network","nvme","option","password",
    "processor","recovery","restore","right","save","secure","security","settings",
    "setup","storage","system","tpm","up","down","usb","value",
)
PHRASE_SPOKEN = (
    "Prêt. Appuyez sur F un pour l'aide.",
    "Flèche haut et flèche bas pour naviguer. Flèches gauche et droite pour modifier.",
    "Entrée pour activer. Échap pour revenir. F un pour l'aide.",
    "Aucun changement.",
    "Aperçu des modifications annulé.",
    "Coché.",
    "Non coché.",
    "Protégé.",
)
