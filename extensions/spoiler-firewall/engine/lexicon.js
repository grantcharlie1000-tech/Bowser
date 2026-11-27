// SFW.lexicon: versioned detection vocabulary. Plain strings, compiled lazily (and cached) through
// the same SFW.normalize pipeline that page text goes through. Synthetic/real spoiler text never
// appears in logs; this file is data only.
//
// Categories (see DESIGN 4.2 + 10):
//   franchise      strong GTA VI references (generated: prefix x numeral, plus explicit forms)
//   franchiseCtx   strong only when an org-level weak term (gta, rockstar, take-two) is also present
//   entityStrong   GTA VI specific names/places
//   weakName/weakOrg/weakPlace/weakRole   ambiguous names (only evidence alongside other signals)
//   cueStrong/cueWeak/marker/game/safe    spoiler cues, markers, game-register and logistics words
//   negative       disambiguators: "phrase>kill1,kill2" suppresses nearby weak entity hits
//   anti           "no spoilers"-style phrases that cancel the overlapping spoiler cue
//   shapeVerb/shapeEnd   cold-context spoiler-shaped sentence parts
SFW.lexicon = (function () {
  const VERSION = "2026.11.18-1";
  const L = (s) => s.split("|");

  // ---------------------------------------------------------------- franchise
  const FRANCHISE_GROUPS = [
    {
      p: L("gta|g t a|grand theft auto|gran theft auto|grand theft auto:|gee tee ay|jee tee ay|gee tee a|jee tee a|gtaa|ggta|gtta|그랜드 theft auto|그랜드 thef auto"),
      n: L(
        "6|vi|vl|six|sechs|seis|sei|zes|alti|szesc|sixth|шесть|шестая|六|陸|육|륙|ستة|หก|εξι|v i|" +
          "06|006|no 6|no6|nr 6|num 6|number 6|n 6|the 6th|the sixth|6th|vith|roman six|roman 6|roman vi|sx|sics|sicks|sixx|siks|бi|бі|" +
          "шість|шiсть|шест|שש|छह|छः|enam|kuusi|kuus|seks|sesi|sase|sest|sesc|shesh|ششه|سته|ست"
      ),
    },
    { p: L("جاتا|جتا|قراند|قراند ثفت اوتو|قراند ثفت أوتو|جراند ثفت أوتو"), n: L("6|vi|ستة|سته|ست") },
    { p: L("ग्रैंड थेफ्ट ऑटो|ग्रैन्ड थेफ्ट ऑटो|ग्रेंड थेफ्ट ऑटो"), n: L("6|vi|छह|छः") },
    { p: L("гта|гэ тэ а"), n: L("six|шість|шiсть|шест|шестой|шести|бi|бі") },
    { p: L("гта|грэнд тефт авто|гранд тефт авто|гранд тэфт авто|гта онлайн"), n: L("6|vi|шесть|шестая|шестой|6я") },
    { p: L("グランド・セフト・オート|グラセフ|ジーティーエー|ジーティーエイ|ジーティーエー"), n: L("6|vi|六|ろく|ロク") },
    { p: L("侠盗猎车手|俠盜獵車手|侠盗飞车|俠盜飛車|侠盗猎车|gta"), n: L("6|vi|六|陆") },
    { p: L("지타|지티에이|그랜드 테프트 오토|그랜드테프트오토"), n: L("6|vi|육|륙") },
    { p: L("جي تي اي|جي تي ايه|جراند ثفت اوتو|جراند ثيفت اوتو"), n: L("6|vi|ستة|٦") },
    { p: L("จีทีเอ|แกรนด์ เธฟต์ ออโต้|แกรนด์ เธฟท์ ออโต้"), n: L("6|vi|หก|๖") },
    { p: L("γτα|γκραντ θεφτ ωτο|γκραντ θεφτ οτο"), n: L("6|vi|εξι") },
    { p: L("जीटीए"), n: L("6|vi|छह") },
  ];
  const FRANCHISE_EXTRA = L(
    "gta sex|vice city 2|vice city ii|vice city two|vc2|grand theft auto sex|gta sexta"
  );
  // soft forms are strong, but a nearby Greater-Toronto-Area disambiguator ("toronto", "gta housing"...) cancels them
  const FRANCHISE_SOFT = L(
    "new gta|next gta|nuevo gta|nuevo grand theft auto|nouveau gta|neues gta|neuer gta|nuovo gta|novo gta|nieuwe gta|nowe gta|nowa gta|yeni gta|" +
      "новая гта|новый гта|новую гта|новой гта|新gta|新作gta|新作グラセフ|새로운 gta|gta 새"
  );
  const FRANCHISE_CTX = L("6ix|the sixth one|sixth one|gta sixx|g6ta");
  // more nicknames, strong like the soft forms ("the new Grand Theft Auto")
  const FRANCHISE_SOFT2 = L(
    "new grand theft auto|next grand theft auto|upcoming grand theft auto|latest grand theft auto|newest grand theft auto|" +
      "sixth gta|6th gta|sixth grand theft auto|6th grand theft auto|gta the sixth|new vice city|next vice city|florida gta|the florida gta|" +
      "neues grand theft auto|nouveau grand theft auto|nuevo grand theft auto|nuovo grand theft auto"
  );
  // strong only with a strong spoiler cue in the same unit (ambiguous alone): ordinary-word numerals and generic nicknames
  const FRANCHISE_CUE = L(
    "gt a6|gta sau|gta sis|gta hat|gta ses|gta sies|grand theft auto sau|grand theft auto sis|" +
      "latest gta|newest gta|upcoming gta|gta next|the next gta|that gta|this gta|the latest grand theft auto|rockstar next game|r next game|" +
      "new rockstar game|upcoming rockstar game|latest rockstar game|newest rockstar game|rockstar new game|rockstars new game|rockstar upcoming game|rockstar latest game|" +
      "rockstar's new game|rockstar's next game|rockstar's upcoming game|rockstar's latest game|rockstar's newest game|r's next game|r's new game|" +
      "6 gta|six gta|vi gta|6 grand theft auto|six grand theft auto|that rockstar game|the rockstar game|rockstar game|" +
      "r new game|new r game|the sixth one|vice city game"
  );
  // as above but only inside the launch window / a hot context (generic "the game from the 19th" shapes live in LAUNCH_DATE)
  const FRANCHISE_LAUNCH = L("the new game|the new rockstar|the vice city game");
  // launch-day date nicknames: a date term + a release/game word + a strong cue, launch window only
  const LAUNCH_DATE = L("the 19th|nov 19|nov 19th|november 19|november 19th|11 19|19 nov|19 november|19th of november|19 11|19th|nov the 19th");
  const LAUNCH_REL = L("game|games|release|releases|releasing|released|launch|launches|launching|drop|drops|dropping|comes out|coming out|come out|out on|dropped");

  // ---------------------------------------------------------------- entities
  const ENTITY_STRONG = L(
    "lucia caminos|jason duval|leonida|leonida keys|port gellhorn|grassrivers|grass rivers|mount kalaga|mount kalaga national park|" +
      "cal hampton|boobie ike|dre'quan priest|dre quan priest|real dimez|raul bautista|brian heder|only raw records|" +
      "люсия каминос|лусия каминос|джейсон дюваль|джейсон дувал|порт геллхорн|маунт калага|" +
      "ルシア・カミノス|ジェイソン・デュバル|ジェイソン・デュヴァル|レオニダ|ポート・ゲルホーン|" +
      "露西亚·卡米诺斯|杰森·杜瓦尔|露西亞卡米諾斯|傑森杜瓦爾|莱昂尼达|萊昂尼達|" +
      "루시아 카미노스|제이슨 두발|레오니다|لوسيا كامينوس|جيسون دوفال|ليونيدا|" +
      "джейсон дюваль|джейсона дюваля|джейсон дюваль|лусия каминос|люсия каминос|леониды|порт гелхорн|порт гелльхорн|маунт калага|" +
      "ジェイソン・デュバル|ジェイソン・デュヴァル|ルシア・カミノス|レオニダ|ポート・ゲルホーン|ポートゲルホーン|" +
      "利奥尼达|利奧尼達|莱昂尼达|萊昂尼達|莱奥尼达|里奥尼达|杰森·杜瓦尔|杰森杜瓦尔|傑森·杜瓦爾|露西亚·卡米诺斯|露西亞·卡米諾斯|" +
      "제이슨 듀발|루시아 카미노스|레오니다 키스|레오니다|لوسيا|" +
      "lucía caminos|lucia caminos|port gelhorn|port gelhorne|port gellhorne|leónida|leonida|" +
      "lucia kaminos|lucia caminoss|jason duvall"
  );
  const WEAK_NAME = L(
    "jason|lucia|cal|raul|boobie|brian|dre'quan|dre quan|duval|" +
      "джейсон|джейсона|джейсону|джейсоном|джейсоне|люсия|люсии|люсию|люсией|лусия|" +
      "ジェイソン|ルシア|杰森|傑森|露西亚|露西亞|卢西亚|盧西亞|露西娅|제이슨|루시아|جيسون|لوسيا|เจสัน|ลูเซีย"
  );
  // low-information names: only anchors when the context is already warm
  const WEAK_NAME_LOW = L("cal|raul|brian|boobie");
  // rare enough that "<name> + a story cue" in the launch window points at the game (Jason is everyone's colleague)
  const WEAK_NAME_DISTINCT = L("lucia|duval|dre'quan|dre quan|люсия|люсии|люсию|люсией|лусия|ルシア|露西亚|露西亞|卢西亚|盧西亞|露西娅|루시아|لوسيا|ลูเซีย");
  const WEAK_ORG = L("gta|grand theft auto|gta online|rockstar|rockstar games|rockstar north|take two|take two interactive|take-two");
  const WEAK_PLACE = L("vice city|ambrosia|леонида");
  const WEAK_ROLE = L(
    "female protag|female protagonist|female lead|the protagonists|both protagonists|the two protagonists|the duo|bonnie and clyde|bonnie clyde|protag|protags"
  );

  // ---------------------------------------------------------------- cues
  const CUE_STRONG = L(
    // en
    "spoiler|spoilers|spoiled|spoils|spoilt|spoil|spoilery|ending|endings|at the end|in the end|the end|final mission|last mission|final cutscene|last cutscene|" +
      "dies|die|died|dying|death|deaths|kills|killed|killing|murdered|murders|betray|betrays|betrayed|betrayal|twist|twists|plot|plots|plot twist|" +
      "leak|leaks|leaked|leaking|leaker|leakers|epilogue|post credits|post credit|who dies|ending explained|full playthrough|all cutscenes|turns out|" +
      "survives|survived|finale|final boss|last boss|true ending|good ending|bad ending|secret ending|alternate ending|the ending|story leak|plot leak|" +
      "reveals that|revealed that|assassinated|executed|sacrifices|sacrificed|story details|plot details|" +
      // es
      "el final|final del juego|muere|mueren|muerte|murio|matan|mata a|traicion|traiciona|traiciono|trama|giro argumental|giro de guion|desenlace|epilogo|" +
      "filtracion|filtraciones|filtrado|filtrada|se revela|revelacion|mision final|ultima mision|" +
      // fr
      "fin du jeu|la fin|meurt|meurent|mort|trahison|trahit|trahi|denouement|revelation|fuite|fuites|rebondissement|derniere mission|mission finale|" +
      // de
      "das ende|stirbt|sterben|tod|getotet|verrat|verraet|handlung|geleakt|enthullung|abspann|letzte mission|spielende|" +
      // it
      "il finale|muore|muoiono|morte|tradimento|tradisce|colpo di scena|fine del gioco|finale del gioco|ultima missione|rivelazione|trapelato|trapelata|" +
      // pt
      "o final|final do jogo|morre|morrem|traicao|enredo|reviravolta|vazamento|vazou|vazado|missao final|ultima missao|desfecho|" +
      // ru
      "спойлер|спойлеры|спойлеров|спойлеры|концовка|концовку|концовки|концовке|финал|финале|умирает|умрет|умирают|погибает|погибнет|погиб|смерть|смерти|" +
      "предательство|предаст|предает|слив|слили|утечка|утечки|эпилог|последняя миссия|финальная миссия|развязка|" +
      // ja
      "ネタバレ|ねたばれ|結末|エンディング|ラスト|死ぬ|死亡|死んだ|殺される|裏切り|裏切る|どんでん返し|リーク|流出|最終ミッション|ラストミッション|エピローグ|黒幕|真相|" +
      // zh
      "剧透|劇透|结局|結局|大结局|死亡|死去|背叛|反转|反轉|泄露|洩漏|爆料|结尾|結尾|最终任务|最終任務|" +
      // ko
      "스포|스포일러|결말|엔딩|죽는다|죽음|죽는|죽어|배신|반전|유출|줄거리|최종 미션|마지막 미션|" +
      // ar
      "حرق|سبويلر|النهاية|نهاية|يموت|موت|مقتل|خيانة|تسريب|حبكة|المهمة الاخيرة|" +
      // th
      "สปอยล์|สปอย|ตอนจบ|ฉากจบ|ตาย|หักหลัง|รั่ว|หลุด|พล็อต|" +
      // tr
      "ihanet|sizinti|sizdirildi|oyunun sonu|son gorev|oldurulur|" +
      // pl
      "spoilery|koniec|zakonczenie|umiera|zginie|ginie|smierc|zdrada|zdradza|fabula|przeciek|wyciek|epilog|ostatnia misja|finalowa misja|" +
      // nl
      "einde|het einde|sterft|doodgaan|verraad|verraadt|plotwending|uitgelekt|laatste missie|epiloog|" +
      // vi / id
      "ket thuc|cai chet|phan boi|ro ri|tiet lo|bocoran|pengkhianat|tamat|misi terakhir"
  );
  const CUE_WEAK = L(
    "story|storyline|mission|missions|character|characters|cutscene|cutscenes|chapter|chapters|boss|villain|villains|reveal|reveals|revealed|campaign|" +
      "protagonist|protagonists|final|last|end|ends|ended|fate|secret|secrets|truth|identity|shoots|shot|killer|romance|relationship|backstory|lore|" +
      "side quest|heist|rumor|rumour|rumors|rumours|datamine|datamined|insider|kill|dead|" +
      "historia|mision|misiones|personaje|escena|capitulo|jefe|villano|campana|fin|protagonista|" +
      "histoire|personnage|scene|chapitre|mechant|campagne|" +
      "geschichte|charakter|figur|zwischensequenz|kapitel|bosskampf|kampagne|ende|" +
      "storia|missione|personaggio|capitolo|cattivo|campagna|fine|" +
      "missao|personagem|cena|capitulo|chefe|vilao|campanha|" +
      "история|миссия|персонаж|катсцена|глава|босс|злодей|кампания|конец|сюжет|" +
      "ストーリー|ミッション|キャラ|ムービー|ボス|最後|最終|" +
      "剧情|劇情|任务|任務|角色|过场|過場|章节|最终|最終|" +
      "스토리|미션|캐릭터|컷신|보스|마지막|최종|" +
      "قصة|مهمة|شخصية|مشهد|زعيم|اخير|نهائي|" +
      "hikaye|gorev|karakter|ara sahne|bolum|kampanya|son|final|" +
      "misja|postac|rozdzial|kampania|" +
      "verhaal|missie|personage|filmpje|hoofdstuk|baas|schurk|campagne"
  );
  const MARKER = L(
    "spoiler alert|spoilers ahead|spoiler warning|major spoilers|big spoilers|massive spoilers|contains spoilers|spoilers below|spoiler below|spoilers inside|" +
      "dont read if|do not read if|read at your own risk|spoiler tag|tagged spoiler|spoiler thread|" +
      "alerte spoiler|alerta de spoiler|spoiler vorsicht|spoilerwarnung|attenzione spoiler|alerta spoiler|спойлер алерт|осторожно спойлеры|" +
      "ネタバレ注意|ネタバレあり|剧透警告|劇透警告|有剧透|스포 주의|스포주의|تحذير سبويلر"
  );
  const ANTI = L("no spoilers|no spoiler|spoiler free|spoilers free|without spoilers|non spoiler|zero spoilers|spoiler less|sin spoilers|sans spoilers|ohne spoiler|senza spoiler|sem spoilers|без спойлеров|ネタバレなし|无剧透|無劇透|스포 없|بدون سبويلر");
  const GAME = L(
    "mission|missions|cutscene|cutscenes|playthrough|play through|walkthrough|chapter|chapters|boss fight|side quest|side quests|100 percent|full completion|" +
      "campaign|story mode|main story|checkpoint|credits|final mission|heist|ミッション|миссия|миссии"
  );
  const SAFE = L(
    "release date|launch date|release time|launch time|price|prices|pre order|preorder|pre orders|preorders|preload|pre load|download size|file size|install size|storage|" +
      "platforms|platform|ps5|ps 5|playstation 5|xbox|series x|series s|switch 2|trailer 1|trailer 2|trailer 3|trailer one|trailer two|pc port|pc version|pc release|" +
      "countdown|can't wait|cant wait|cannot wait|so excited|hyped|hype|review score|reviews|review|metacritic|opencritic|sales|units sold|delay|delayed|midnight|" +
      "unlock time|unlock times|what time|how long until|days left|days until|hours until|system requirements|specs|collectors edition|special edition|editions|bundle|" +
      "digital|physical copy|steelbook|gift card|discount|walmart|best buy|gamestop|amazon|fecha de lanzamiento|date de sortie|erscheinungsdatum|data di uscita|data de lancamento|" +
      "дата выхода|発売日|发售日|發售日|출시일|تاريخ الاصدار"
  );
  const SHAPE_VERB_X = L(
    // en: gerunds, nouns, idioms
    "dying|death|deaths|doesnt survive|doesnt make it|didnt survive|didnt make it|wont survive|wont make it|cant survive|doesn't survive|doesn't make it|didn't survive|didn't make it|won't survive|won't make it|can't survive|gets killed off|killed off|kill off|kills off|" +
      "betrayal|betraying|betrays|being the mole|is the mole|was the mole|being the rat|was the rat|being the traitor|was the traitor|is the snitch|" +
      "surviving|survival|working with the cops|working with the feds|has been dead|been dead the whole|is revealed as|revealed as the|is the real|gets betrayed|got betrayed|" +
      "was working with|is working with|turns out to be|turned out to be|ends up dead|ends up dying|is secretly|was secretly|" +
      // es
      "muere|mueren|murio|morira|traiciona|traiciono|es la traidora|es el traidor|sobrevive|es la rata|" +
      // fr
      "meurt|meurent|trahit|est la traitre|est le traitre|survit|" +
      // de
      "stirbt|sterben|verraet|verrat|ist der verrater|ueberlebt|" +
      // it
      "muore|muoiono|tradisce|e il traditore|sopravvive|" +
      // pt
      "morre|morrem|trai|e a traidora|e o traidor|sobrevive|" +
      // ru
      "умирает|умрет|умирают|погибает|погибнет|предает|предаст|предатель|предательница|выживает|убит|убита|убивают|" +
      // ja zh ko
      "死ぬ|死亡|死んだ|殺される|裏切る|裏切り|死去|被杀|被殺|背叛|죽는다|죽음|죽어|죽는|배신|" +
      // ar
      "يموت|تموت|خيانة|يخون"
  );
  const SHAPE_END_X = L(
    "the whole game|whole game|the game ending|game ending|ending of the game|finale of the game|end of the game|final heist|last heist|the big twist|big twist|good twist|such a twist|the twist|plot twist|twist ending|" +
      "act three|act 3|act two|act 2|act four|act 4|act five|act 5|act six|act 6|final act|last act|" +
      "al final|en el final|final del juego|ultima mision|mision final|en los creditos|" +
      "a la fin|fin du jeu|derniere mission|mission finale|" +
      "am ende|im finale|letzte mission|" +
      "alla fine|nel finale|ultima missione|" +
      "no final|no fim|ultima missao|" +
      "в финале|в конце|в концовке|в последней миссии|в эпилоге|на финале|" +
      "最後|最終|ラスト|結末|エンディング|最终|最后|结局|結局|大结局|结尾|" +
      "결말|엔딩|마지막 미션|최종 미션|" +
      "النهاية|المهمة الاخيرة"
  );
  // "<name> is the rat/traitor/..." forms that are a spoiler by themselves in the launch window
  const SHAPE_IDENT = L(
    "is the rat|is the cop|is the traitor|is the snitch|is the mole|is the villain|is a rat|is a cop|is a traitor|is a snitch|is the killer|is the final boss|" +
      "was the rat|was the cop|was the traitor|was the mole|was the snitch|was the villain|being the mole|being the rat|being the traitor|being the villain|being the cop|" +
      "es la rata|es la traidora|es el traidor|est la traitre|est le traitre|ist der verrater|e a traidora|e o traidor|предатель|предательница|предал|предала"
  );
  // game-register words that make an unnamed "X dies at the end/in the final mission" a game spoiler
  const GAME_STRICT = L(
    "mission|missions|cutscene|cutscenes|playthrough|play through|walkthrough|boss|boss fight|final boss|side quest|side quests|campaign|story mode|checkpoint|dlc|speedrun|trophy|achievement|heist|" +
      "миссия|миссии|миссией|ミッション|任务|任務|미션|مهمة|mision|missao|missione"
  );
  // other works whose names/spoilers are not ours: kill weak-name signals and the generic shape nearby
  const OTHER_WORKS = L(
    "far cry|resident evil|silent hill|the last of us|god of war|mass effect|elden ring|dark souls|red dead|the witcher|witcher 3|skyrim|cyberpunk|bioshock|metal gear|final fantasy|zelda|" +
      "assassins creed|call of duty|half life|uncharted|fire emblem|octopath|baldurs gate|dragon age|the sims|minecraft|pokemon|hollow knight|starfield|" +
      "saints row|watch dogs|just cause|sleeping dogs|payday|borderlands|diablo|overwatch|" +
      "opera|donizetti|verdi|puccini|medea|argonauts|greek myth|shakespeare|season finale|series finale|netflix|hbo|disney|marvel|dc comics|" +
      "friday the 13th|freddy vs jason|family guy|american dad|the simpsons|south park|bobs burgers|rick and morty|the office|grey s anatomy|greys anatomy|the walking dead|nightmare on elm|slasher|the sopranos|breaking bad|game of thrones|stranger things|squid game|yellowjackets|oppenheimer|" +
      "telenovela|novela|pelicula|filme|film|movie|horrorfilm|horrorfilms|sitcom|tv show|anime|manga|映画|13日の金曜日|电影|電影|영화|фильм|фильме|сериал|" +
      "the warriors|manhunt|max payne|la noire|midnight club|album|singer|song|concert|energy drink|restaurant|chef|" +
      "rockstar energy|rockstar records"
  );
  // real-life context: "my cousin Jason", "our group chat" (names right next to these are people we know, not characters)
  const REAL_LIFE = L(
    "my cousin|my brother|my sister|my friend|my dad|my mom|my mum|my uncle|my aunt|my son|my daughter|my cat|my dog|my coworker|my roommate|my neighbor|my neighbour|my teacher|my boss|my husband|my wife|my boyfriend|my girlfriend|" +
      "our group chat|group chat|leftovers"
  );
  const SHAPE_VERB = L(
    "dies|die|died|is killed|gets killed|get killed|was killed|got killed|killed|betrays|betrayed|betray|is the rat|is the cop|is the traitor|is the snitch|" +
      "is the villain|is a rat|is a cop|is a traitor|is a snitch|survives|survived|was working for|is working for|works for|worked for|shoots|shot|turns on|" +
      "sacrifices|gets shot|goes to prison|gets arrested|gets caught|is revealed to be|is the one who"
  );
  const SHAPE_END = L(
    "at the end|in the end|the end|ending|the ending|final mission|last mission|credits|post credits|final cutscene|last cutscene|finale|epilogue|final boss|" +
      "last chapter|final chapter|last scene|final scene|very end|last act|final act|last level|final level|story ends"
  );
  // the specific "this is a spoiler/leak" cues (not plain "ending"/"dies"/"the end")
  const LEAK_CUES = L(
    "spoiler|spoilers|spoiled|spoilt|spoil|spoilery|leak|leaks|leaked|leaking|leaker|leakers|post credits|post credit|epilogue|ending explained|plot twist|final mission|last mission|story leak|plot leak|true ending|secret ending|" +
      "alternate ending|who dies|full playthrough|all cutscenes|story details|plot details|final cutscene|last cutscene|final boss|last boss|" +
      "ネタバレ|ねたばれ|剧透|劇透|스포|스포일러|спойлер|спойлеры|слив|слили|утечка|filtracion|filtrado|filtrada|fuite|geleakt|trapelato|vazamento|vazou|سبويلر|تسريب|สปอยล์|สปอย"
  );
  // the explicit "spoiler" words (subset of LEAK_CUES)
  const SPOILER_WORDS = L("spoiler|spoilers|spoiled|spoilt|spoil|spoilery|ネタバレ|ねたばれ|剧透|劇透|스포|스포일러|спойлер|спойлеры|سبويلر|สปอยล์|สปอย|spoilers");
  const UNIT_WORDS = L("hours|hour|hrs|hr|minutes|minute|mins|min|times|percent|pm|am|kg|km|mph|lbs|inches|feet|days|weeks|people|players");
  const FUZZY_STOP = L("leonidas|leonid|leonidov|leonidovich|leonidis|leonia|leonie|leoni|leonida's");

  // phrase>kills (comma separated); kills default to every weak term found inside the phrase.
  const NEGATIVE = L(
    // Jason
    "jason voorhees|voorhees|friday the 13th|jason bourne|bourne identity|jason statham|statham|jason todd|jason momoa|momoa|jason kelce|kelce|jason derulo|derulo|" +
      "jason mraz|mraz|jason sudeikis|sudeikis|jason segel|segel|jason isaacs|isaacs|jason lee|jason alexander|jason bateman|jason schwartzman|jason reitman|jason blum|" +
      "jason aldean|jason orange|jason donovan|jason robards|jason priestley|jason biggs|jason clarke|jason mantzoukas|jason witten|jason kidd|jason giambi|jason heyward|" +
      "jason day|jason dufner|jason collins|jason taylor|jason terry|jason williams|jason varitek|jason castro|jason david frank|jason ritter|jason ralph|jason sehorn|" +
      "jason and the argonauts|jason the argonaut|argonauts|jason mccartney|jason manford|jason bonham|jason newsted|jason roy|jason holder|jason behrendorff|jason gillespie|" +
      "джейсон вурхиз|джейсон стэйтем|джейсон борн|джейсон момоа|ジェイソン・ステイサム|ジェイソン・ボーン|ジェイソン・ボーヒーズ|ジェイソン・モモア|杰森·斯坦森|杰森·莫玛|杰森·伯恩|杰森·莫玛|" +
      // Lucia
      "st lucia|st. lucia|saint lucia|sainte lucie|sainte-lucie|santa lucia|santa lucía|lucia di lammermoor|lammermoor|lucia joyce|lucia berlin|lucia moniz|lucia hiriart|" +
      "lucia mendez|lucia aniello|lucia pamela|lucia rijker|lucia lucia|saint lucy|lucia bose|lucia gonzalez|lucia fernandez|lucia annunziata|lucia chiara|lucia mondella|" +
      // Rockstar / GTA
      "rockstar energy|rockstar energy drink|rockstar records|rockstar lounge|rock star energy|" +
      "gta v|gta 5|gta iv|gta 4|gta iii|gta 3|gta ii|gta 2|gta san andreas|san andreas|gta vice city|vice city stories|vice city 2002|vice city stories|" +
      "grand theft auto v|grand theft auto 5|grand theft auto iv|grand theft auto 4|grand theft auto iii|grand theft auto 3|grand theft auto ii|grand theft auto 2|" +
      "grand theft auto san andreas|grand theft auto vice city|grand theft auto liberty city|gta liberty city|liberty city stories|chinatown wars|" +
      "greater toronto area|gta housing|gta traffic|gta real estate|gta market|gta homes|gta rental|gta rentals|toronto>gta|mississauga>gta|brampton>gta|durham region>gta|" +
      "york region>gta|peel region>gta|halton region>gta|ontario>gta|gta area|gta condo|gta condos|gta weather|gta transit|gta residents|gta schools|" +
      // Cal / Raul / Brian / Duval
      "duval county|duval street|cal ripken|cal poly|cal state|cal berkeley|cal bears|cal thomas|cal raleigh|cal crutchlow|cal fire|cal worthington|" +
      "raul castro|raul julia|raul gonzalez|raul jimenez|raul ruidiaz|raul garcia|raul alcala|brian may|brian cox|brian wilson|brian eno|brian johnson|brian williams|brian kelly|" +
      "brian cranston|brian laundrie|brian griffin|brian adams|brian de palma|brian regan|brian posehn|brian tyree henry|brian austin green|brian molko|brian jones|" +
      "boobie miles|boobie gibbs|boobie clarke|boobie trap|booby trap|" +
      // roles
      "bonnie and clyde the movie|bonnie and clyde 1967|the two protagonists of"
  );

  const DATA = {
    version: VERSION,
    franchiseGroups: FRANCHISE_GROUPS,
    franchiseExtra: FRANCHISE_EXTRA,
    franchiseSoft: FRANCHISE_SOFT,
    franchiseCtx: FRANCHISE_CTX,
    entityStrong: ENTITY_STRONG,
    weakName: WEAK_NAME,
    weakNameLow: WEAK_NAME_LOW,
    weakOrg: WEAK_ORG,
    weakPlace: WEAK_PLACE,
    weakRole: WEAK_ROLE,
    cueStrong: CUE_STRONG,
    cueWeak: CUE_WEAK,
    marker: MARKER,
    anti: ANTI,
    game: GAME,
    safe: SAFE,
    shapeVerb: SHAPE_VERB,
    shapeEnd: SHAPE_END,
    shapeVerbX: SHAPE_VERB_X,
    shapeEndX: SHAPE_END_X,
    shapeIdent: SHAPE_IDENT,
    gameStrict: GAME_STRICT,
    otherWorks: OTHER_WORKS,
    franchiseSoft2: FRANCHISE_SOFT2,
    franchiseCue: FRANCHISE_CUE,
    franchiseLaunch: FRANCHISE_LAUNCH,
    launchDate: LAUNCH_DATE,
    launchRel: LAUNCH_REL,
    unitWords: UNIT_WORDS,
    realLife: REAL_LIFE,
    spoilerWords: SPOILER_WORDS,
    leakCues: LEAK_CUES,
    fuzzyStop: FUZZY_STOP,
    negative: NEGATIVE,
  };

  // ---------------------------------------------------------------- feed extensions (add-only)
  const EXT = { franchise: [], entityStrong: [], entityWeak: [], cueStrong: [], cueWeak: [], marker: [], game: [], shapeVerb: [], shapeEnd: [] };
  const EXT_TARGET = { franchise: "franchise", entityStrong: "entityStrong", entityWeak: "weakName", cueStrong: "cueStrong", cueWeak: "cueWeak", marker: "marker", game: "game", shapeVerb: "shapeVerb", shapeEnd: "shapeEnd" };
  const RANK = { entityWeak: 1, entityStrong: 2, franchise: 3, cueWeak: 1, cueStrong: 2, marker: 3 };
  const FAMILY = { entityWeak: "e", entityStrong: "e", franchise: "e", cueWeak: "c", cueStrong: "c", marker: "c" };
  const LIMITS = { maxTerms: 5000, maxTermLen: 64, maxJson: 200000 };
  let extCount = 0;
  let extVersion = "";
  let compiledCache = null;

  const hasOwn = Object.prototype.hasOwnProperty;
  function validTerm(t) {
    return typeof t === "string" && t.length >= 2 && t.length <= LIMITS.maxTermLen && !/[\u0000-\u001f]/.test(t);
  }

  // Adds terms / raises a term's category. Never removes or lowers anything. Returns a summary.
  function extend(feed) {
    const res = { ok: false, added: 0, rejected: 0, reason: "" };
    try {
      if (!feed || typeof feed !== "object") { res.reason = "feed"; return res; }
      let size = 0;
      try { size = JSON.stringify(feed).length; } catch (e) { res.reason = "json"; return res; }
      if (size > LIMITS.maxJson) { res.reason = "size"; return res; }
      const add = hasOwn.call(feed, "add") && feed.add && typeof feed.add === "object" ? feed.add : {};
      const raise = hasOwn.call(feed, "raise") && feed.raise && typeof feed.raise === "object" ? feed.raise : {};
      const staged = [];
      for (const cat of Object.keys(EXT)) {
        if (!hasOwn.call(add, cat) || !Array.isArray(add[cat])) continue;
        for (const t of add[cat]) {
          if (!validTerm(t)) { res.rejected++; continue; }
          staged.push([cat, t]);
        }
      }
      const cur = compile();
      for (const t of Object.keys(raise)) {
        const to = raise[t];
        if (!validTerm(t) || typeof to !== "string" || !hasOwn.call(RANK, to)) { res.rejected++; continue; }
        const key = SFW.normalize.term(t).c;
        let from = 0;
        for (const cat of ["entityWeak", "entityStrong", "franchise", "cueWeak", "cueStrong", "marker"]) {
          const set = cur.rankSets[cat];
          if (set && set.has(key) && FAMILY[cat] === FAMILY[to]) from = Math.max(from, RANK[cat]);
        }
        if (RANK[to] > from) staged.push([to, t]);
        else res.rejected++;
      }
      if (extCount + staged.length > LIMITS.maxTerms) { res.reason = "limit"; return res; }
      for (const [cat, t] of staged) {
        EXT[cat].push(t);
        res.added++;
        extCount++;
      }
      if (typeof feed.version === "string" && feed.version.length <= 40 && /^[\w.+-]+$/.test(feed.version)) extVersion = feed.version;
      if (res.added) {
        compiledCache = null;
        api.version = VERSION + (extVersion ? "+" + extVersion : "");
      }
      res.ok = true;
    } catch (e) {
      res.reason = "error";
    }
    return res;
  }

  // ---------------------------------------------------------------- compilation
  function newNode() {
    return { c: Object.create(null), t: null };
  }
  function insert(root, term) {
    let node = root;
    const s = term.c;
    for (let i = 0; i < s.length; i++) {
      const ch = s.charAt(i);
      let nx = node.c[ch];
      if (!nx) nx = node.c[ch] = newNode();
      node = nx;
    }
    (node.t || (node.t = [])).push(term);
  }

  function esc(s) {
    return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  }
  function anchorRegex(list) {
    const root = { c: Object.create(null), end: false };
    for (const a of list) {
      if (!a) continue;
      let node = root;
      for (let i = 0; i < a.length && !node.end; i++) {
        const ch = a.charAt(i);
        node = node.c[ch] || (node.c[ch] = { c: Object.create(null), end: false });
      }
      node.end = true;
    }
    function re(node) {
      if (node.end) return "";
      const keys = Object.keys(node.c);
      if (!keys.length) return "";
      const parts = keys.map((k) => esc(k) + re(node.c[k]));
      return parts.length === 1 ? parts[0] : "(?:" + parts.join("|") + ")";
    }
    const src = re(root);
    return src ? new RegExp(src) : /(?!)/;
  }

  const RARITY = { h: 1, l: 1, d: 1, c: 1, u: 1, m: 2, f: 2, p: 2, g: 2, w: 2, y: 2, b: 3, v: 3, k: 3, x: 4, j: 4, q: 4, z: 4 };
  // frequent English letter groups: poor prefilter anchors
  const COMMON_FRAGMENTS = ["moun", "ount", "port", "real", "rive", "iver", "vers", "tain", "ment", "tion", "ther", "ould", "with", "ough", "ever", "ound", "ming", "inos", "cord", "park", "hamp"];
  function rareWindows(c, count, k, minFirst) {
    const chosen = [];
    for (let n = 0; n < count; n++) {
      let best = -1, bi = -1, bc = 0, bestKey = -1;
      for (let i = 0; i + k <= c.length; i++) {
        let ok = true;
        for (const j of chosen) if (i < j + k + 1 && j < i + k + 1) ok = false; // keep a gap of one char
        if (!ok || COMMON_FRAGMENTS.indexOf(c.slice(i, i + k)) >= 0) continue;
        let sc = 0, cnt = 0;
        for (let q = 0; q < k; q++) {
          const w = RARITY[c.charAt(i + q)] || 0;
          sc += w;
          if (w) cnt++;
        }
        if (sc * 10 + cnt >= bestKey) { bestKey = sc * 10 + cnt; best = sc; bi = i; bc = cnt; }
      }
      if (bi < 0) break;
      if (minFirst > 1) { if (n === 0 ? best < 4 && bc < 3 : best < 3 || bc < 2) break; } else if (best < minFirst) break; // only rare-letter windows keep the prefilter cheap
      chosen.push(bi);
    }
    return chosen.map((i) => c.slice(i, i + k));
  }

  // d+1 disjoint windows with a one-letter gap between them: d edits (insert/delete/substitute/transpose)
  // cannot touch all of them
  function editWindows(c, d) {
    const n = c.length;
    const L2 = Math.floor((n - d) / (d + 1));
    if (L2 < 3) return [];
    const out = [];
    for (let i = 0; i <= d; i++) {
      const start = i === d ? n - L2 : i * (L2 + 1);
      out.push(c.slice(start, start + L2));
    }
    return out;
  }

  function vowelDrop(s) {
    return s.replace(/(^|\s)([a-z])([a-z]*)/g, (m, sp, a, rest) => sp + a + rest.replace(/[aeiou]/g, ""));
  }

  // ---------------------------------------------------------------- build-time snapshot
  // compile() runs in every document and every frame before the first text can be judged, and cold (no JIT yet) it was
  // ~30 ms of main-thread work per document on a fast PC, ~120 ms on a slow laptop: most of the engine's cost on pages that
  // never mention the game. build/build.mjs runs compile() once, encodes the result (encodeSnapshot) and embeds it after this
  // file (setSnapshot). In the page, compile() revives the snapshot instead: the same terms, inserted in the same order into
  // fresh tries, the same Sets and the same RegExp sources. It is used only for the built-in vocabulary (no feed terms) of
  // this exact VERSION, and only after the page's own normaliser reproduced the snapshot's sample keys (Unicode / ICU drift
  // between Node and the browser falls back to compiling). Any failure falls back to compiling, never to fewer terms.
  const SNAP_FORMAT = 1;
  let snapText = null;
  let fromSnap = false; // the cached compiled lexicon came from the snapshot (tests, diagnostics)
  // the last compile()'s bookkeeping for encodeSnapshot: { lex, order: terms in add() order, rr: Set of terms in trieRR }.
  // A plain variable, not a WeakMap: nothing here may touch WeakMap / WeakSet while the bundle loads (a page that broke
  // them before the engine must still get the runtime's fail-closed path, not a bundle that throws).
  let lastMeta = null;

  function setSnapshot(text) {
    snapText = typeof text === "string" ? text : null;
  }

  function snapshotSamples() {
    // raw lexicon strings whose key depends on the normaliser's Unicode tables (non-ASCII), plus a few plain ones
    const out = [];
    const seen = new Set();
    const pushAll = (v) => {
      if (typeof v === "string") { if (!seen.has(v)) { seen.add(v); out.push(v); } } else if (Array.isArray(v)) v.forEach(pushAll);
      else if (v && typeof v === "object") Object.keys(v).forEach((k) => pushAll(v[k]));
    };
    pushAll(DATA);
    const wide = out.filter((s) => /[^\x00-\x7f]/.test(s));
    const pick = [];
    const blocks = new Set();
    for (const s of wide) { const b = s.codePointAt(s.search(/[^\x00-\x7f]/)) >> 6; if (!blocks.has(b)) { blocks.add(b); pick.push(s); } }
    for (let i = 0; pick.length < 40 && i < wide.length; i += 7) if (pick.indexOf(wide[i]) < 0) pick.push(wide[i]);
    return pick.concat(["grand theft auto 6", "Vice City", "g t a v i"]);
  }

  function encodeSnapshot() {
    const saved = compiledCache;
    compiledCache = null;
    let lex;
    const savedSnap = snapText;
    snapText = null;
    try { lex = compile(); } finally { snapText = savedSnap; }
    compiledCache = saved || lex;
    if (extCount) throw new Error("snapshot of an extended lexicon");
    const meta = lastMeta && lastMeta.lex === lex ? lastMeta : null;
    if (!meta) throw new Error("snapshot: compile bookkeeping missing");
    // Row: [shape, c, offs (0 for [0], else "a,b"), flags (bit 0: in trieRR; bit i+1: i-th boolean of the shape), ...rest].
    // A shape holds the key order, cat, kind, which keys are booleans and which are "rest" (kills -> kill table index, near).
    const shapes = [], shapeIx = new Map(), kills = [], killIx = new Map(), terms = [];
    const FIXED = { c: 1, n: 1, offs: 1, cat: 1, kind: 1 };
    for (const t of meta.order) {
      const keys = Object.keys(t);
      if (t.n !== t.c.length || typeof t.kind !== "string" || typeof t.cat !== "string") throw new Error("snapshot: unexpected term " + t.c);
      const bools = [], rest = [];
      for (const k of keys) {
        if (FIXED[k]) continue;
        const v = t[k];
        if (typeof v === "boolean") bools.push(k);
        else if (k === "kills" && v instanceof Set) rest.push(k);
        else if (k === "near" && typeof v === "number" && isFinite(v)) rest.push(k);
        else throw new Error("snapshot: cannot encode term field " + k);
      }
      const sk = keys.join(",") + "|" + t.cat + "|" + t.kind + "|" + bools.join(",") + "|" + rest.join(",");
      if (!shapeIx.has(sk)) { shapeIx.set(sk, shapes.length); shapes.push({ k: keys, cat: t.cat, kind: t.kind, b: bools, r: rest }); }
      let flags = meta.rr.has(t) ? 1 : 0;
      for (let i = 0; i < bools.length; i++) if (t[bools[i]]) flags |= 2 << i;
      const offs = t.offs.length === 1 && t.offs[0] === 0 ? 0 : t.offs.join(",");
      const row = [shapeIx.get(sk), t.c, offs, flags];
      for (const k of rest) {
        if (k === "near") { row.push(t.near); continue; }
        const arr = Array.from(t.kills);
        const kk = JSON.stringify(arr);
        if (!killIx.has(kk)) { killIx.set(kk, kills.length); kills.push(arr); }
        row.push(killIx.get(kk));
      }
      terms.push(row);
    }
    const sets = (o) => { const r = {}; for (const k of Object.keys(o)) r[k] = Array.from(o[k]); return r; };
    const N = SFW.normalize;
    return JSON.stringify({
      f: SNAP_FORMAT, v: VERSION,
      check: snapshotSamples().map((s) => { const tm = N.term(s); return [s, tm.c, tm.offs.join(","), !!tm.sub]; }),
      shapes, kills, terms,
      fuzzy: lex.fuzzy, unitSet: Array.from(lex.unitSet), stopSet: Array.from(lex.stopSet), rankSets: sets(lex.rankSets),
      anchorBase: lex.anchorBase.source, anchorHot: lex.anchorHot.source, anchorRR: lex.anchorRR.source, anchorTypo: lex.anchorTypo.source,
      hasRR: lex.hasRR,
    });
  }

  function revive(text) {
    const s = JSON.parse(text);
    if (!s || s.f !== SNAP_FORMAT || s.v !== VERSION) return null;
    const N = SFW.normalize;
    for (const [raw, c, offs, sub] of s.check) {
      const tm = N.term(raw);
      if (tm.c !== c || tm.offs.join(",") !== offs || !!tm.sub !== sub) return null;
    }
    const killSets = s.kills.map((a) => new Set(a)); // read-only (classify only calls has())
    const trieTok = newNode(), trieSub = newNode(), trieRR = newNode();
    const byCat = {};
    // one template per shape holding its keys in compile()'s order: a term is a copy of it with the values filled in (the
    // same keys in the same order as compile() makes; this runs cold in every document, so it does as little as it can)
    const tpls = s.shapes.map((sh) => { const o = {}; for (let i = 0; i < sh.k.length; i++) o[sh.k[i]] = undefined; return o; });
    for (const row of s.terms) {
      const sh = s.shapes[row[0]];
      const c = row[1], flags = row[3];
      const t = Object.assign({}, tpls[row[0]]);
      t.c = c; t.n = c.length; t.offs = row[2] === 0 ? [0] : row[2].split(",").map(Number); t.cat = sh.cat; t.kind = sh.kind;
      for (let i = 0; i < sh.b.length; i++) t[sh.b[i]] = (flags & (2 << i)) !== 0;
      for (let i = 0; i < sh.r.length; i++) t[sh.r[i]] = sh.r[i] === "kills" ? killSets[row[4 + i]] : row[4 + i];
      (byCat[t.cat] || (byCat[t.cat] = [])).push(t);
      insert(t.sub ? trieSub : trieTok, t);
      if (flags & 1) insert(trieRR, t);
    }
    const rankSets = {};
    for (const k of Object.keys(s.rankSets)) rankSets[k] = new Set(s.rankSets[k]);
    return {
      version: api.version,
      trieTok, trieSub, trieRR,
      byCat, fuzzy: s.fuzzy, unitSet: new Set(s.unitSet), stopSet: new Set(s.stopSet), rankSets,
      anchorBase: new RegExp(s.anchorBase),
      anchorHot: new RegExp(s.anchorHot),
      anchorRR: new RegExp(s.anchorRR),
      anchorTypo: new RegExp(s.anchorTypo),
      hasRR: s.hasRR,
    };
  }

  function compile() {
    if (compiledCache) return compiledCache;
    if (snapText && !extCount) {
      let c = null;
      try { c = revive(snapText); } catch (e) { c = null; }
      if (c) { compiledCache = c; fromSnap = true; return c; }
      snapText = null; // unusable here: compile from the word lists (now and after any later feed)
    }
    fromSnap = false;
    const N = SFW.normalize;
    const order = [];
    const rr = new Set();
    const trieTok = newNode(), trieSub = newNode(), trieRR = newNode();
    const byCat = {};
    const rankSets = { entityWeak: new Set(), entityStrong: new Set(), franchise: new Set(), cueWeak: new Set(), cueStrong: new Set(), marker: new Set() };
    const anchorsBase = [], anchorsRR = [], anchorsHot = [], anchorsTypo = [];
    const fuzzy = [];
    const seen = Object.create(null);
    const weakKeys = new Set();
    const negatives = [];

    function mk(str, cat, extra) {
      const tm = N.term(str);
      if (!tm.c || tm.c.length < 1) return null;
      const t = { c: tm.c, n: tm.c.length, offs: tm.offs, sub: tm.sub, cat: cat, kind: "", ctx: false, unit: false, vi: false };
      if (extra) for (const k in extra) t[k] = extra[k];
      return t;
    }
    function add(str, cat, extra) {
      const t = mk(str, cat, extra);
      if (!t) return null;
      const key = cat + "\u0000" + t.c + "\u0000" + (t.kind || "") + "\u0000" + t.offs.join(",") + (t.launch ? "L" : "");
      if (seen[key]) return null;
      seen[key] = 1;
      (byCat[cat] || (byCat[cat] = [])).push(t);
      insert(t.sub ? trieSub : trieTok, t);
      order.push(t);
      return t;
    }
    function anchorOf(t, hot) {
      let a;
      if (t.sub) a = N.skFold(t.c.slice(0, Math.min(t.c.length, t.c.charCodeAt(0) < 128 ? 8 : 3)));
      else a = N.skFold(t.c.slice(0, Math.min(t.c.length, 8)));
      if (!a || (!t.sub && a.length < 3)) return;
      (hot ? anchorsHot : anchorsBase).push(a);
    }
    function rrAnchor(t) {
      if (t.sub || t.n < 4 || (t.n < 5 && !/\d/.test(t.c))) return;
      const f = N.skFold(t.c.slice(0, 8));
      const r = N.skFold(N.reverse(t.c).slice(0, 8));
      const o = N.skFold(N.rot13(t.c).slice(0, 8));
      if (f.length >= 4) anchorsRR.push(r, o);
      if (/[69]/.test(t.c)) anchorsRR.push(N.skFold(N.flip(t.c).slice(0, 8)));
      if (t.offs.length > 1) {
        let pw = "";
        for (let i = 0; i < t.offs.length; i++) pw += N.reverse(t.c.slice(t.offs[i], i + 1 < t.offs.length ? t.offs[i + 1] : t.n));
        anchorsRR.push(N.skFold(pw.slice(0, 8)));
      }
    }

    function franchise(str, ctxOnly, soft) {
      const t = add(str, ctxOnly ? "franchiseCtx" : "franchise", { unit: false, soft: !!soft });
      if (!t) return;
      if (/(?:6|six)$/.test(t.c)) t.unit = true;
      if (/vi$/.test(t.c)) t.vi = true;
      t.dn = /[0-9]$/.test(t.c);
      rankSets.franchise.add(t.c);
      // soft nicknames that contain gta / grand theft auto / vice city are reached through those anchors ("upcoming" is no anchor)
      if (!(soft && /gta|grand theft|vice city/.test(str))) anchorOf(t, false);
      if (!ctxOnly && !t.sub) { insert(trieRR, t); rr.add(t); rrAnchor(t); }
    }
    function all(list) { return list.concat(); }

    // franchise
    const groups = FRANCHISE_GROUPS;
    for (const g of groups) {
      for (const p of g.p) {
        const ptoks = p.trim().split(/\s+/).length;
        // "GTA V I loved it" must not read as "GTA VI": the spaced Roman numeral only follows spaced letters/words
        for (const nn of g.n) {
          if (nn === "v i" && ptoks < 3) continue;
          franchise(p + " " + nn, false);
          if (nn === "v i" && /^(?:[a-z] )+[a-z]$/.test(p)) { const t = byCat.franchise && byCat.franchise[byCat.franchise.length - 1]; if (t) t.ss = true; }
        }
      }
    }
    for (const s of FRANCHISE_EXTRA.concat(EXT.franchise)) franchise(s, false);
    for (const s of FRANCHISE_SOFT.concat(FRANCHISE_SOFT2)) franchise(s, false, true);
    // ambiguous numerals / generic nicknames: strong only together with a strong spoiler cue (launch-only ones need the launch/hot prior too)
    for (const [list, launch] of [[FRANCHISE_CUE, false], [FRANCHISE_LAUNCH, true]]) {
      for (const str of list) {
        const t = add(str, "fcue", { launch: launch });
        // terms that contain gta / rockstar / vice city / grand theft auto are reached through those anchors already
        if (t && !/gta|rockstar|vice city|grand theft|^r /.test(str)) anchorOf(t, launch);
      }
    }
    for (const str of LAUNCH_DATE) {
      const t = add(str, "ldate");
      if (!t) continue;
      // anchor on the digits end of the date ("ber19"), never on the month word alone
      if (/nov|11/.test(str)) { const di = t.c.search(/[0-9]/), st0 = Math.max(0, di - 2), a = N.skFold(t.c.slice(st0, st0 + 5)); if (a) anchorsBase.push(a); } else anchorOf(t, true);
    }
    for (const str of LAUNCH_REL) { const t = add(str, "lrel"); if (t) anchorOf(t, true); }
    for (const s of FRANCHISE_CTX) franchise(s, true);
    // The franchise name itself is fuzzy-matched; the numeral must then match exactly.
    {
      const nm = N.term("grand theft auto").c;
      const numerals = [];
      for (const nn of FRANCHISE_GROUPS[0].n) {
        const c = N.term(nn).c;
        if (c && !N.term(nn).sub) numerals.push(c);
      }
      fuzzy.push({ c: nm, d: 1, first: nm.charAt(0), nums: numerals, cat: "franchise" });
      const h = Math.floor(nm.length / 2);
      anchorsBase.push(N.skFold(nm.slice(0, h)), N.skFold(nm.slice(h)));
    }

    // entities
    for (const s of ENTITY_STRONG.concat(EXT.entityStrong)) {
      const t = add(s, "entity");
      if (!t) continue;
      rankSets.entityStrong.add(t.c);
      anchorOf(t, false);
      if (!t.sub) {
        insert(trieRR, t);
        rr.add(t);
        rrAnchor(t);
        if (t.n >= 7 && t.offs.length >= 1 && /^[a-z]+$/.test(t.c)) {
          const d = t.n >= 11 ? 2 : 1;
          fuzzy.push({ c: t.c, d: d, first: t.c.charAt(0), nums: null, cat: "entity" });
          if (t.n >= 9) {
            // Two (three for d=2) disjoint, rare-letter windows: a single edit or transposition
            // cannot destroy windows separated by a gap, so a one-edit variant keeps at least one.
            for (const w of rareWindows(t.c, d === 2 ? 3 : 2, 4, 4)) anchorsBase.push(N.skFold(w));
          } else {
            for (const w of rareWindows(t.c, 1, 4, 1)) anchorsBase.push(N.skFold(w));
          }
          // typo-tolerant anchor: d+1 disjoint windows separated by a one-char gap; a text that only has a
          // misspelling of this name passes only the cheap fuzzy check (bit 64), never the full classifier.
          for (const w of editWindows(t.c, 1)) anchorsTypo.push(N.skFold(w));
        }
        // vowel-dropped forms of multi-word names: "jsn dvl", "lc cmns"
        if ((/ /.test(s) || t.n >= 8) && /^[a-z' ]+$/i.test(s)) {
          const vd = vowelDrop(s.toLowerCase().replace(/'/g, ""));
          const t2 = vd.replace(/ /g, "").length >= 5 ? add(vd, "entity") : null;
          if (t2) { t2.vd = true; anchorOf(t2, false); }
        }
      }
    }
    // weak entities
    function weak(list, kind, hotOnly) {
      for (const s of list) {
        const t = add(s, "weak", { kind: kind, low: hotOnly.has(s), distinct: (kind === "name" || kind === "place") && distinctSet.has(s) });
        if (!t) continue;
        weakKeys.add(t.c);
        rankSets.entityWeak.add(t.c);
        anchorOf(t, hotOnly.has(s));
      }
    }
    const lowSet = new Set(WEAK_NAME_LOW), noneSet = new Set(), distinctSet = new Set(WEAK_NAME_DISTINCT.concat(["vice city", "леонида"]));
    weak(WEAK_NAME.concat(EXT.entityWeak), "name", lowSet);
    weak(WEAK_ORG, "org", noneSet);
    weak(WEAK_PLACE, "place", noneSet);
    weak(WEAK_ROLE, "role", noneSet);

    // cues
    const strongKeys = new Set();
    const leakSet = new Set(LEAK_CUES.map((w) => N.term(w).c));
    const spoilerSet = new Set(SPOILER_WORDS.map((w) => N.term(w).c));
    for (const s of CUE_STRONG.concat(EXT.cueStrong)) {
      const t = add(s, "cueS");
      if (!t) continue;
      t.lk = leakSet.has(t.c);
      t.ls = spoilerSet.has(t.c);
      strongKeys.add(t.c);
      rankSets.cueStrong.add(t.c);
      anchorOf(t, true);
    }
    for (const s of CUE_WEAK.concat(EXT.cueWeak)) {
      const t = mk(s, "cueW");
      if (!t || strongKeys.has(t.c)) continue;
      add(s, "cueW");
      rankSets.cueWeak.add(t.c);
      anchorOf(t, true);
    }
    for (const s of MARKER.concat(EXT.marker)) {
      const t = add(s, "marker");
      if (t) { rankSets.marker.add(t.c); anchorOf(t, false); }
    }
    for (const s of ANTI) { const t = add(s, "anti"); if (t) anchorOf(t, true); }
    for (const s of GAME.concat(EXT.game)) add(s, "game");
    for (const s of SAFE) add(s, "safe");
    for (const s of SHAPE_VERB.concat(SHAPE_VERB_X, EXT.shapeVerb)) add(s, "sverb");
    for (const s of SHAPE_END.concat(SHAPE_END_X, EXT.shapeEnd)) add(s, "send");
    for (let i = 1; i <= 40; i++) add("mission " + i, "send");
    for (const s of SHAPE_IDENT) add(s, "sident");
    for (const s of GAME_STRICT) add(s, "gstrict");

    // negatives: kills = explicit "phrase>a,b" or the weak terms contained (token aligned) in the phrase
    const weakTerms = byCat.weak || [];
    for (const raw of NEGATIVE) {
      const gt = raw.indexOf(">");
      const phrase = gt < 0 ? raw : raw.slice(0, gt);
      const t = add(phrase, "neg");
      if (!t) continue;
      const kills = new Set();
      if (gt >= 0) {
        for (const k of raw.slice(gt + 1).split(",")) kills.add(N.term(k).c);
      } else {
        const ends = t.offs.map((o, i) => (i + 1 < t.offs.length ? t.offs[i + 1] : t.n));
        for (const w of weakTerms) {
          let from = 0, idx;
          while ((idx = t.c.indexOf(w.c, from)) >= 0) {
            const e = idx + w.c.length;
            if (t.offs.indexOf(idx) >= 0 && ends.indexOf(e) >= 0) { kills.add(w.c); break; }
            from = idx + 1;
          }
        }
      }
      t.kills = kills;
    }

    {
      const allWeak = new Set(weakTerms.map((w) => w.c));
      allWeak.add("*");
      for (const w of OTHER_WORKS) {
        let t = add(w, "neg");
        if (!t) {
          const c = N.term(w).c;
          t = (byCat.neg || []).find((x) => x.c === c) || null;
        }
        if (t) t.kills = new Set([...(t.kills || []), ...allWeak]);
      }
    }

    for (const w of REAL_LIFE) {
      const t = add(w, "neg");
      if (t) { t.kills = new Set(weakTerms.filter((x) => x.kind === "name").map((x) => x.c)); t.near = 40; }
    }
    for (const raw of L("gta like>gta|gta clone>gta|gta killer>gta|gta style>gta")) {
      const gt = raw.indexOf(">");
      const t = add(raw.slice(0, gt), "neg");
      if (t) t.kills = new Set([N.term(raw.slice(gt + 1)).c]);
    }

    // numeric franchise terms: next-token unit suppression and "vi" split protection are flags above
    const unitSet = new Set(UNIT_WORDS.map((w) => N.term(w).c));
    const stopSet = new Set(FUZZY_STOP.map((w) => N.term(w).c));

    compiledCache = {
      version: api.version,
      trieTok, trieSub, trieRR,
      byCat, fuzzy, unitSet, stopSet, rankSets,
      anchorBase: anchorRegex(anchorsBase),
      anchorHot: anchorRegex(anchorsHot),
      anchorRR: anchorRegex(anchorsRR),
      anchorTypo: anchorsTypo.length ? anchorRegex(anchorsTypo) : /(?!)/,
      hasRR: anchorsRR.length > 0,
    };
    lastMeta = { lex: compiledCache, order, rr };
    return compiledCache;
  }

  const api = {
    version: VERSION,
    data: DATA,
    compile,
    compiled: compile,
    extend,
    encodeSnapshot,
    setSnapshot,
    fromSnapshot: () => fromSnap,
    limits: LIMITS,
  };
  return api;
})();
