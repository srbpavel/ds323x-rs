//! Alarm support

use super::{decimal_to_packed_bcd, hours_to_register, packed_bcd_to_decimal};
use crate::{
    ds323x::{NaiveTime, Timelike},
    interface::{ReadData, WriteData},
    BitFlags, Ds323x, Error, Hours, Register,
};

/// Parameters for setting Alarm1 on a day of the month
///
/// Depending on the matching strategy, some fields may not be relevant. In this
/// case, invalid values are ignored and the minimum valid values are used instead to
/// configure the alarm:
/// - Second, minute and hour: 0
/// - Day: 1
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DayAlarm1 {
    /// Day of the month [1-31]
    pub day: u8,
    /// Hour
    pub hour: Hours,
    /// Minute [0-59]
    pub minute: u8,
    /// Second [0-59]
    pub second: u8,
}

/// Parameters for setting Alarm1 on a weekday
///
/// Depending on the matching strategy, some fields may not be relevant. In this
/// case, invalid values are ignored and the minimum valid values are used instead to
/// configure the alarm:
/// - Second, minute and hour: 0
/// - Weekday: 1
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeekdayAlarm1 {
    /// Weekday [1-7]
    pub weekday: u8,
    /// Hour
    pub hour: Hours,
    /// Minute [0-59]
    pub minute: u8,
    /// Second [0-59]
    pub second: u8,
}

/// Alarm1 trigger rate
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Alarm1Matching {
    /// Alarm once per second.
    OncePerSecond,
    /// Alarm when seconds match.
    SecondsMatch,
    /// Alarm when minutes and seconds match.
    MinutesAndSecondsMatch,
    /// Alarm when hours, minutes and seconds match.
    HoursMinutesAndSecondsMatch,
    /// Alarm when date/weekday, hours, minutes and seconds match.
    AllMatch,
}

/// Parameters for setting Alarm2 on a day of the month
///
/// Depending on the matching strategy, some fields may not be relevant. In this
/// case, invalid values are ignored and the minimum valid values are used instead to
/// configure the alarm:
/// - Minute and hour: 0
/// - Day: 1
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DayAlarm2 {
    /// Day of month [1-31]
    pub day: u8,
    /// Hour
    pub hour: Hours,
    /// Minute [0-59]
    pub minute: u8,
}

/// Parameters for setting Alarm2 on a weekday
///
/// Depending on the matching strategy, some fields may not be relevant. In this
/// case, invalid values are ignored and the minimum valid values are used instead to
/// configure the alarm:
/// - Minute and hour: 0
/// - Weekday: 1
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeekdayAlarm2 {
    /// Weekday [1-7]
    pub weekday: u8,
    /// Hour
    pub hour: Hours,
    /// Minute [0-59]
    pub minute: u8,
}

/// Alarm2 trigger rate
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Alarm2Matching {
    /// Alarm once per minute. (00 seconds of every minute)
    OncePerMinute,
    /// Alarm when minutes match.
    MinutesMatch,
    /// Alarm when hours and minutes match.
    HoursAndMinutesMatch,
    /// Alarm when date/weekday, hours and minutes match.
    AllMatch,
}

fn get_matching_mask_alarm1(matching: Alarm1Matching) -> [u8; 4] {
    const AM: u8 = BitFlags::ALARM_MATCH;
    match matching {
        Alarm1Matching::OncePerSecond => [AM, AM, AM, AM],
        Alarm1Matching::SecondsMatch => [0, AM, AM, AM],
        Alarm1Matching::MinutesAndSecondsMatch => [0, 0, AM, AM],
        Alarm1Matching::HoursMinutesAndSecondsMatch => [0, 0, 0, AM],
        Alarm1Matching::AllMatch => [0, 0, 0, 0],
    }
}

fn get_matching_mask_alarm2(matching: Alarm2Matching) -> [u8; 3] {
    const AM: u8 = BitFlags::ALARM_MATCH;
    match matching {
        Alarm2Matching::OncePerMinute => [AM, AM, AM],
        Alarm2Matching::MinutesMatch => [0, AM, AM],
        Alarm2Matching::HoursAndMinutesMatch => [0, 0, AM],
        Alarm2Matching::AllMatch => [0, 0, 0],
    }
}

/// Test if hour value is valid
fn is_hour_valid(hours: Hours) -> bool {
    match hours {
        Hours::H24(h) if h > 23 => true,
        Hours::AM(h) if !(1..=12).contains(&h) => true,
        Hours::PM(h) if !(1..=12).contains(&h) => true,
        _ => false,
    }
}

/// Amend invalid hour values
fn amend_hour(hours: Hours) -> Hours {
    match hours {
        Hours::H24(h) if h > 23 => Hours::H24(0),
        Hours::H24(h) => Hours::H24(h),
        Hours::AM(h) if !(1..=12).contains(&h) => Hours::AM(1),
        Hours::AM(h) => Hours::AM(h),
        Hours::PM(h) if !(1..=12).contains(&h) => Hours::PM(1),
        Hours::PM(h) => Hours::PM(h),
    }
}

/// Helper functions for parsing alarm register data
fn is_24h_format(hour_register: u8) -> bool {
    (hour_register & BitFlags::H24_H12) == 0
}

fn is_am(hour_register: u8) -> bool {
    (hour_register & BitFlags::AM_PM) == 0
}

fn hours_from_register(data: u8) -> Hours {
    if is_24h_format(data) {
        Hours::H24(packed_bcd_to_decimal(data & !BitFlags::H24_H12))
    } else if is_am(data) {
        Hours::AM(packed_bcd_to_decimal(
            data & !(BitFlags::H24_H12 | BitFlags::AM_PM),
        ))
    } else {
        Hours::PM(packed_bcd_to_decimal(
            data & !(BitFlags::H24_H12 | BitFlags::AM_PM),
        ))
    }
}

impl<DI, IC, E> Ds323x<DI, IC>
where
    DI: ReadData<Error = Error<E>> + WriteData<Error = Error<E>>,
{
    /// Set Alarm1 for day of the month.
    ///
    /// Will return an `Error::InvalidInputData` if any of the used parameters
    /// (depending on the matching startegy) is out of range. Any unused
    /// parameter is set to the corresponding minimum valid value:
    /// - Second, minute, hour: 0
    /// - Day: 1
    pub fn set_alarm1_day(
        &mut self,
        when: DayAlarm1,
        matching: Alarm1Matching,
    ) -> Result<(), Error<E>> {
        let day_invalid = when.day < 1 || when.day > 31;
        let hour_invalid = is_hour_valid(when.hour);
        let minute_invalid = when.minute > 59;
        let second_invalid = when.second > 59;

        let day = if day_invalid { 1 } else { when.day };
        let hour = amend_hour(when.hour);
        let minute = if minute_invalid { 0 } else { when.minute };

        if (matching == Alarm1Matching::AllMatch && (day_invalid || hour_invalid))
            || (hour_invalid && matching == Alarm1Matching::HoursMinutesAndSecondsMatch)
            || ((matching != Alarm1Matching::SecondsMatch
                && matching != Alarm1Matching::OncePerSecond)
                && minute_invalid)
            || second_invalid
        {
            return Err(Error::InvalidInputData);
        }

        let match_mask = get_matching_mask_alarm1(matching);
        let mut data = [
            Register::ALARM1_SECONDS,
            decimal_to_packed_bcd(when.second) | match_mask[0],
            decimal_to_packed_bcd(minute) | match_mask[1],
            hours_to_register(hour)? | match_mask[2],
            decimal_to_packed_bcd(day) | match_mask[3],
        ];
        self.iface.write_data(&mut data)
    }

    /// Set Alarm1 for a time (fires when hours, minutes and seconds match).
    ///
    /// Will return an `Error::InvalidInputData` if any of the parameters is out of range.
    /// The day is not used by this matching strategy and is set to 1.
    pub fn set_alarm1_hms(&mut self, when: NaiveTime) -> Result<(), Error<E>> {
        let alarm = DayAlarm1 {
            day: 1,
            hour: Hours::H24(when.hour() as u8),
            minute: when.minute() as u8,
            second: when.second() as u8,
        };
        self.set_alarm1_day(alarm, Alarm1Matching::HoursMinutesAndSecondsMatch)
    }

    /// Set Alarm1 for weekday.
    ///
    /// Will return an `Error::InvalidInputData` if any of the used parameters
    /// (depending on the matching startegy) is out of range. Any unused
    /// parameter is set to the corresponding minimum valid value:
    /// - Second, minute, hour: 0
    /// - Weekday: 1
    pub fn set_alarm1_weekday(
        &mut self,
        when: WeekdayAlarm1,
        matching: Alarm1Matching,
    ) -> Result<(), Error<E>> {
        let weekday_invalid = when.weekday < 1 || when.weekday > 7;
        let hour_invalid = is_hour_valid(when.hour);
        let minute_invalid = when.minute > 59;
        let second_invalid = when.second > 59;

        let weekday = if weekday_invalid { 1 } else { when.weekday };
        let hour = amend_hour(when.hour);
        let minute = if minute_invalid { 0 } else { when.minute };
        let second = if second_invalid { 0 } else { when.second };

        if ((hour_invalid || weekday_invalid) && matching == Alarm1Matching::AllMatch)
            || (hour_invalid && matching == Alarm1Matching::HoursMinutesAndSecondsMatch)
            || (minute_invalid
                && (matching != Alarm1Matching::OncePerSecond
                    && matching != Alarm1Matching::SecondsMatch))
            || (second_invalid && matching != Alarm1Matching::OncePerSecond)
        {
            return Err(Error::InvalidInputData);
        }
        let match_mask = get_matching_mask_alarm1(matching);
        let mut data = [
            Register::ALARM1_SECONDS,
            decimal_to_packed_bcd(second) | match_mask[0],
            decimal_to_packed_bcd(minute) | match_mask[1],
            hours_to_register(hour)? | match_mask[2],
            decimal_to_packed_bcd(weekday) | match_mask[3] | BitFlags::WEEKDAY,
        ];
        self.iface.write_data(&mut data)
    }

    /// Set Alarm2 for date (day of month).
    ///
    /// Will return an `Error::InvalidInputData` if any of the used parameters
    /// (depending on the matching startegy) is out of range. Any unused
    /// parameter is set to the corresponding minimum valid value:
    /// - Minute, hour: 0
    /// - Day: 1
    pub fn set_alarm2_day(
        &mut self,
        when: DayAlarm2,
        matching: Alarm2Matching,
    ) -> Result<(), Error<E>> {
        let day_invalid = when.day < 1 || when.day > 31;
        let hour_invalid = is_hour_valid(when.hour);
        let minute_invalid = when.minute > 59;

        let day = if day_invalid { 1 } else { when.day };
        let hour = amend_hour(when.hour);
        let minute = if minute_invalid { 0 } else { when.minute };

        if ((day_invalid || hour_invalid) && matching == Alarm2Matching::AllMatch)
            || (hour_invalid && matching == Alarm2Matching::HoursAndMinutesMatch)
            || (matching != Alarm2Matching::OncePerMinute && minute_invalid)
        {
            return Err(Error::InvalidInputData);
        }

        let match_mask = get_matching_mask_alarm2(matching);
        let mut data = [
            Register::ALARM2_MINUTES,
            decimal_to_packed_bcd(minute) | match_mask[0],
            hours_to_register(hour)? | match_mask[1],
            decimal_to_packed_bcd(day) | match_mask[2],
        ];
        self.iface.write_data(&mut data)
    }

    /// Set Alarm2 for a time (fires when hours and minutes match).
    ///
    /// Will return an `Error::InvalidInputData` if any of the parameters is out of range.
    /// The day is not used by this matching strategy and is set to 1.
    pub fn set_alarm2_hm(&mut self, when: NaiveTime) -> Result<(), Error<E>> {
        let alarm = DayAlarm2 {
            day: 1,
            hour: Hours::H24(when.hour() as u8),
            minute: when.minute() as u8,
        };
        self.set_alarm2_day(alarm, Alarm2Matching::HoursAndMinutesMatch)
    }

    /// Set Alarm2 for weekday.
    ///
    /// Will return an `Error::InvalidInputData` if any of the used parameters
    /// (depending on the matching startegy) is out of range. Any unused
    /// parameter is set to the corresponding minimum valid value:
    /// - Minute, hour: 0
    /// - Weekday: 1
    pub fn set_alarm2_weekday(
        &mut self,
        when: WeekdayAlarm2,
        matching: Alarm2Matching,
    ) -> Result<(), Error<E>> {
        let weekday_invalid = when.weekday < 1 || when.weekday > 7;
        let hour_invalid = is_hour_valid(when.hour);
        let minute_invalid = when.minute > 59;

        let weekday = if weekday_invalid { 1 } else { when.weekday };
        let hour = amend_hour(when.hour);
        let minute = if minute_invalid { 0 } else { when.minute };

        if (matching == Alarm2Matching::AllMatch && (weekday_invalid || hour_invalid))
            || (matching == Alarm2Matching::HoursAndMinutesMatch && hour_invalid)
            || (minute_invalid && matching != Alarm2Matching::OncePerMinute)
        {
            return Err(Error::InvalidInputData);
        }
        let match_mask = get_matching_mask_alarm2(matching);
        let mut data = [
            Register::ALARM2_MINUTES,
            decimal_to_packed_bcd(minute) | match_mask[0],
            hours_to_register(hour)? | match_mask[1],
            decimal_to_packed_bcd(weekday) | match_mask[2] | BitFlags::WEEKDAY,
        ];
        self.iface.write_data(&mut data)
    }

    /// Read Alarm1 configuration from DS3231 registers (addresses 07h-0Ah)
    ///
    /// Returns the current Alarm1 configuration if enabled, or None if disabled.
    ///
    /// # DS3231 Alarm Behavior
    ///
    /// - **Alarm registers always contain values** - they cannot be "empty"
    /// - **Mask bits** (bit 7 in each register) determine which fields are compared:
    ///   - 0 = field is compared (alarm enabled for this field)
    ///   - 1 = field is ignored (mask bit set)
    /// - **All mask bits set** = alarm effectively disabled
    /// - **Interrupt enable** (separate from this function) controls if alarm can fire
    ///
    /// # Alarm Types Detected
    ///
    /// Based on mask bit patterns:
    /// - `HoursMinutesAndSecondsMatch`: Daily alarm (fires every day at specific time)
    /// - `AllMatch`: Day-specific or weekday alarm (determined by DY/DT bit)
    /// - Other patterns: Special matching strategies
    ///
    /// # Returns
    ///
    /// - `Ok(Some((alarm, matching)))` - Alarm configuration and matching strategy
    /// - `Ok(None)` - All mask bits set (alarm disabled)
    /// - `Err(_)` - I2C communication error
    pub fn read_alarm1_config(&mut self) -> Result<Option<(DayAlarm1, Alarm1Matching)>, Error<E>> {
        // Read all 4 alarm1 registers: seconds, minutes, hours, day/date
        let mut alarm_regs = [0u8; 4];

        // Read individual registers
        alarm_regs[0] = self.iface.read_register(Register::ALARM1_SECONDS)?;
        alarm_regs[1] = self.iface.read_register(Register::ALARM1_MINUTES)?;
        alarm_regs[2] = self.iface.read_register(Register::ALARM1_HOURS)?;
        alarm_regs[3] = self.iface.read_register(Register::ALARM1_DAY_DATE)?;

        // Parse the alarm configuration
        self.parse_alarm1_registers(&alarm_regs)
    }

    /// Read Alarm2 configuration from DS3231 registers (addresses 0Bh-0Dh)
    ///
    /// Returns the current Alarm2 configuration if enabled, or None if disabled.
    ///
    /// # DS3231 Alarm2 Behavior
    ///
    /// Alarm2 is similar to Alarm1 but **has no seconds precision** - only minutes and hours.
    ///
    /// - **Alarm registers always contain values** - they cannot be "empty"
    /// - **Mask bits** (bit 7 in each register) determine which fields are compared
    /// - **All mask bits set** = alarm effectively disabled
    /// - **Interrupt enable** (separate from this function) controls if alarm can fire
    ///
    /// # Alarm Types Detected
    ///
    /// Based on mask bit patterns:
    /// - `HoursAndMinutesMatch`: Daily alarm (fires every day at specific time)
    /// - `AllMatch`: Day-specific or weekday alarm (determined by DY/DT bit)
    /// - Other patterns: Special matching strategies
    ///
    /// # Returns
    ///
    /// - `Ok(Some((alarm, matching)))` - Alarm configuration and matching strategy
    /// - `Ok(None)` - All mask bits set (alarm disabled)
    /// - `Err(_)` - I2C communication error
    pub fn read_alarm2_config(&mut self) -> Result<Option<(DayAlarm2, Alarm2Matching)>, Error<E>> {
        // Read all 3 alarm2 registers: minutes, hours, day/date
        let mut alarm_regs = [0u8; 3];

        // Read individual registers
        alarm_regs[0] = self.iface.read_register(Register::ALARM2_MINUTES)?;
        alarm_regs[1] = self.iface.read_register(Register::ALARM2_HOURS)?;
        alarm_regs[2] = self.iface.read_register(Register::ALARM2_DAY_DATE)?;

        // Parse the alarm configuration
        self.parse_alarm2_registers(&alarm_regs)
    }

    /// Parse Alarm1 register data into structured configuration
    fn parse_alarm1_registers(&self, regs: &[u8; 4]) -> Result<Option<(DayAlarm1, Alarm1Matching)>, Error<E>> {
        let seconds_reg = regs[0];
        let minutes_reg = regs[1];
        let hours_reg = regs[2];
        let day_date_reg = regs[3];

        // Check mask bits to determine matching strategy
        let seconds_mask = (seconds_reg & BitFlags::ALARM_MATCH) != 0;
        let minutes_mask = (minutes_reg & BitFlags::ALARM_MATCH) != 0;
        let hours_mask = (hours_reg & BitFlags::ALARM_MATCH) != 0;
        let day_date_mask = (day_date_reg & BitFlags::ALARM_MATCH) != 0;

        // Determine matching strategy from mask pattern
        let matching = match (seconds_mask, minutes_mask, hours_mask, day_date_mask) {
            (true, true, true, true) => Alarm1Matching::OncePerSecond, // All masks = every second
            (true, true, true, false) => Alarm1Matching::OncePerSecond,
            (false, true, true, true) => Alarm1Matching::SecondsMatch,
            (false, false, true, true) => Alarm1Matching::MinutesAndSecondsMatch,
            (false, false, false, true) => Alarm1Matching::HoursMinutesAndSecondsMatch,
            (false, false, false, false) => Alarm1Matching::AllMatch,
            // Handle other mask combinations gracefully
            // Default to most restrictive matching that makes sense
            _ => {
                // Determine the most appropriate matching based on which masks are set
                if seconds_mask && minutes_mask && hours_mask {
                    // Most masks set, closest to OncePerSecond
                    Alarm1Matching::OncePerSecond
                } else if minutes_mask && hours_mask {
                    // Seconds not masked, closest to SecondsMatch
                    Alarm1Matching::SecondsMatch
                } else if hours_mask {
                    // Minutes and possibly seconds not masked
                    Alarm1Matching::MinutesAndSecondsMatch
                } else if day_date_mask {
                    // Hours not masked, day might be masked
                    Alarm1Matching::HoursMinutesAndSecondsMatch
                } else {
                    // Default to most specific matching
                    Alarm1Matching::AllMatch
                }
            }
        };

        // Parse time values from BCD
        let second = packed_bcd_to_decimal(seconds_reg & 0x7F);
        let minute = packed_bcd_to_decimal(minutes_reg & 0x7F);
        let hour = hours_from_register(hours_reg & 0x3F); // Remove mask and weekday bits

        // Parse day/date
        let is_weekday = (day_date_reg & BitFlags::WEEKDAY) != 0;
        let day_value = packed_bcd_to_decimal(day_date_reg & 0x3F);

        let alarm = DayAlarm1 {
            day: if is_weekday { day_value } else { day_value }, // Both use same field in DayAlarm1
            hour,
            minute,
            second,
        };

        Ok(Some((alarm, matching)))
    }

    /// Parse Alarm2 register data into structured configuration
    fn parse_alarm2_registers(&self, regs: &[u8; 3]) -> Result<Option<(DayAlarm2, Alarm2Matching)>, Error<E>> {
        let minutes_reg = regs[0];
        let hours_reg = regs[1];
        let day_date_reg = regs[2];

        // Check mask bits to determine matching strategy
        let minutes_mask = (minutes_reg & BitFlags::ALARM_MATCH) != 0;
        let hours_mask = (hours_reg & BitFlags::ALARM_MATCH) != 0;
        let day_date_mask = (day_date_reg & BitFlags::ALARM_MATCH) != 0;

        // Determine matching strategy from mask pattern
        let matching = match (minutes_mask, hours_mask, day_date_mask) {
            (true, true, true) => Alarm2Matching::OncePerMinute, // All masks = every minute
            (true, true, false) => Alarm2Matching::OncePerMinute,
            (false, true, true) => Alarm2Matching::MinutesMatch,
            (false, false, true) => Alarm2Matching::HoursAndMinutesMatch,
            (false, false, false) => Alarm2Matching::AllMatch,
            // Handle other mask combinations gracefully
            // Default to most restrictive matching that makes sense
            _ => {
                // Determine the most appropriate matching based on which masks are set
                if minutes_mask && hours_mask {
                    // Most masks set, closest to OncePerMinute
                    Alarm2Matching::OncePerMinute
                } else if hours_mask {
                    // Minutes not masked, closest to MinutesMatch
                    Alarm2Matching::MinutesMatch
                } else if day_date_mask {
                    // Hours not masked, day might be masked
                    Alarm2Matching::HoursAndMinutesMatch
                } else {
                    // Default to most specific matching
                    Alarm2Matching::AllMatch
                }
            }
        };

        // Parse time values from BCD
        let minute = packed_bcd_to_decimal(minutes_reg & 0x7F);
        let hour = hours_from_register(hours_reg & 0x3F); // Remove mask and weekday bits

        // Parse day/date
        let is_weekday = (day_date_reg & BitFlags::WEEKDAY) != 0;
        let day_value = packed_bcd_to_decimal(day_date_reg & 0x3F);

        let alarm = DayAlarm2 {
            day: if is_weekday { day_value } else { day_value }, // Both use same field in DayAlarm2
            hour,
            minute,
        };

        Ok(Some((alarm, matching)))
    }

    /// Read Alarm1 interrupt enable status from DS3231 Control register
    ///
    /// Returns true if Alarm1 interrupts are enabled, false if disabled.
    /// This determines whether the alarm can actually trigger an interrupt.
    ///
    /// Note: An alarm can be configured but won't fire unless interrupts are enabled.
    pub fn read_alarm1_interrupt_enabled(&mut self) -> Result<bool, Error<E>> {
        let control_reg = self.iface.read_register(Register::CONTROL)?;
        Ok((control_reg & BitFlags::ALARM1_INT_EN) != 0)
    }

    /// Read Alarm2 interrupt enable status from DS3231 Control register
    ///
    /// Returns true if Alarm2 interrupts are enabled, false if disabled.
    /// This determines whether the alarm can actually trigger an interrupt.
    ///
    /// Note: An alarm can be configured but won't fire unless interrupts are enabled.
    pub fn read_alarm2_interrupt_enabled(&mut self) -> Result<bool, Error<E>> {
        let control_reg = self.iface.read_register(Register::CONTROL)?;
        Ok((control_reg & BitFlags::ALARM2_INT_EN) != 0)
    }
}
