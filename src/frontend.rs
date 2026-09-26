//! Shared conservative FR/EN normalization. Ambiguous formats remain literal.
pub fn normalize(text: &str, french: bool) -> String {
    text.split_whitespace().map(|word| normalize_token(word, french)).collect::<Vec<_>>().join(" ")
}
fn normalize_token(token: &str, fr: bool) -> String {
    let core=token.trim_end_matches(['.', ',', ';', '!', '?']);
    let tail=&token[core.len()..];
    let abbr=match (fr,token) {
        (true,"M.")=>Some("monsieur"), (true,"Mme")|(true,"Mme.")=>Some("madame"),
        (true,"Dr.")=>Some("docteur"), (false,"Dr.")=>Some("doctor"),
        (false,"Mr.")=>Some("mister"), (false,"Mrs.")=>Some("missus"), _=>None
    };
    if let Some(a)=abbr { return a.into(); }
    let time=core.split(if core.contains(':') { ':' } else { 'h' }).collect::<Vec<_>>();
    if time.len()==2 {
        if let (Ok(h),Ok(m))=(time[0].parse::<u32>(),time[1].parse::<u32>()) {
            if h<24 && m<60 {
                return if fr { format!("{h} heures {m} minutes{tail}") } else { format!("{h} hours {m} minutes{tail}") };
            }
        }
    }
    let date=core.split(if core.contains('/') {'/'} else {'-'}).collect::<Vec<_>>();
    if date.len()==3 {
        let nums=date.iter().map(|x|x.parse::<u32>()).collect::<Result<Vec<_>,_>>();
        if let Ok(n)=nums {
            let (y,m,d)=if date[0].len()==4 {(n[0],n[1],n[2])} else if fr && date[2].len()==4 {(n[2],n[1],n[0])} else {(0,0,0)};
            let leap=y%4==0 && (y%100!=0 || y%400==0);
            let days=match m {2=>if leap {29}else{28},4|6|9|11=>30,1|3|5|7|8|10|12=>31,_=>0};
            if y>0 && y<=9999 && d>0 && d<=days {
                let months=if fr { ["janvier","février","mars","avril","mai","juin","juillet","août","septembre","octobre","novembre","décembre"] }
                    else {["January","February","March","April","May","June","July","August","September","October","November","December"]};
                let day=if fr && d==1 {"premier".into()}else{d.to_string()};
                return format!("{day} {} {y}{tail}",months[m as usize-1]);
            }
        }
    }
    let separator=if fr {','}else{'.'};
    if let Some((a,b))=core.split_once(separator) {
        if !a.is_empty() && !b.is_empty() && a.bytes().chain(b.bytes()).all(|c|c.is_ascii_digit()) {
            let digits=if fr {["zéro","un","deux","trois","quatre","cinq","six","sept","huit","neuf"]}
                else {["zero","one","two","three","four","five","six","seven","eight","nine"]};
            let fraction=b.bytes().map(|c|digits[(c-b'0') as usize]).collect::<Vec<_>>().join(" ");
            return format!("{a} {} {fraction}{tail}",if fr {"virgule"}else{"point"});
        }
    }
    token.into()
}
