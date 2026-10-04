pub(super) const DAY: i64 = 86_400_000;
// Gregorian civil dates and epoch days; Euclidean division keeps pre-epoch dates correct.
pub(super) fn days(year: i64, month: i64, day: i64) -> i64 {
    let year = year + (month - 1).div_euclid(12);
    let month = (month - 1).rem_euclid(12) + 1;
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = month + if month > 2 { -3 } else { 9 };
    let doy = (153 * m + 2) / 5 + day - 1;
    era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468
}
pub(super) fn civil(day: i64) -> (i64, i64, i64) {
    let z = day + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (y + i64::from(m <= 2), m, d)
}
pub(super) fn weekday(day: i64) -> i64 {
    (day + 3).rem_euclid(7) + 1
}
pub(super) fn first_week(y: i64, m: i64) -> i64 {
    let first = days(y, m, 1);
    let dow = weekday(first);
    first + if dow > 4 { 8 - dow } else { 1 - dow }
}
#[derive(Clone, Copy, Debug)]
pub(super) struct Fields {
    pub year: i64,
    pub month: i64,
    pub day: i64,
    pub epoch_day: i64,
    pub time: i64,
}
impl Fields {
    pub fn new(millis: i64) -> Self {
        let epoch_day = millis.div_euclid(DAY);
        let (year, month, day) = civil(epoch_day);
        Self {
            year,
            month,
            day,
            epoch_day,
            time: millis.rem_euclid(DAY),
        }
    }
    fn week(&self, month: bool) -> (i64, i64, i64) {
        let (mut y, mut m) = if month {
            (self.year, self.month)
        } else {
            (self.year, 1)
        };
        let first = first_week(y, m);
        let (next_y, next_m) = if month {
            if m == 12 { (y + 1, 1) } else { (y, m + 1) }
        } else {
            (y + 1, 1)
        };
        if self.epoch_day < first {
            if month {
                if m == 1 {
                    y -= 1;
                    m = 12;
                } else {
                    m -= 1;
                }
            } else {
                y -= 1;
            }
        } else if self.epoch_day >= first_week(next_y, next_m) {
            y = next_y;
            m = next_m;
        }
        (y, m, (self.epoch_day - first_week(y, m)).div_euclid(7) + 1)
    }
    pub fn get(&self, c: char) -> i64 {
        match c {
            'Y' => self.year,
            'M' => self.month,
            'D' => self.day,
            'd' => self.epoch_day - days(self.year, 1, 1) + 1,
            'F' => weekday(self.epoch_day),
            'W' => self.week(false).2,
            'X' => self.week(false).0,
            'w' => self.week(true).2,
            'x' => self.week(true).1,
            'H' => self.time / 3_600_000,
            'h' => {
                let h = (self.time / 3_600_000) % 12;
                if h == 0 { 12 } else { h }
            }
            'P' => i64::from(self.time >= 43_200_000),
            'm' => self.time / 60_000 % 60,
            's' => self.time / 1000 % 60,
            'f' => self.time % 1000,
            _ => 0,
        }
    }
}
